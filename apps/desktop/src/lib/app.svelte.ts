// App-wide reactive state (Svelte 5 runes). Logic lives in pure modules
// (timeline.ts, modes.ts, shortcuts.ts); this class only wires them to the API.
import type { ModeName } from "@next-agent/contracts";
import { api, type RouterHealthReport, type Workspace } from "./api.ts";
import type { DiffAction } from "./diff.ts";
import { cycleMode } from "./modes.ts";
import {
  applyMessage,
  messageTaskId,
  newTask,
  resolveApproval,
  settleReview,
  type ApprovalChoice,
  type StreamMessage,
  type TaskState,
} from "./timeline.ts";

export type RouterHealth = "ok" | "degraded" | "down" | "unknown";

class AppStore {
  workspaces = $state<Workspace[]>([]);
  activeWorkspaceId = $state<string | null>(null);
  tasks = $state<TaskState[]>([]);
  activeTaskId = $state<string | null>(null);
  mode = $state<ModeName>("manual");
  router = $state<RouterHealth>("unknown");
  routerDetail = $state<string>("");
  error = $state<string | null>(null);

  /** Worktree branch of the active task (auto mode), if any. */
  get branch(): string | null {
    return this.activeTask?.branch ?? null;
  }

  get activeWorkspace(): Workspace | undefined {
    return this.workspaces.find((w) => w.id === this.activeWorkspaceId);
  }
  get activeTask(): TaskState | undefined {
    return this.tasks.find((t) => t.id === this.activeTaskId);
  }
  get pendingCount(): number {
    return this.tasks.reduce((n, t) => n + t.approvals.filter((a) => a.choice === undefined).length, 0);
  }

  private setHealth(report: RouterHealthReport | null): void {
    if (!report) return;
    this.router = report.status;
    this.routerDetail = report.detail;
  }

  async init(): Promise<void> {
    try {
      this.setHealth(await api.routerHealth());
      await api.onRouterHealth((report) => this.setHealth(report));
    } catch {
      // Health is informational; never block startup on it.
    }
    try {
      // Scheduled (cron) runs start in the app itself: show them like any task.
      const adopted = new Set<string>();
      await api.onTaskStarted((started) => {
        if (started.source !== "cron" || this.tasks.some((t) => t.id === started.taskId)) return;
        adopted.add(started.taskId);
        this.tasks = [{ ...newTask(started.taskId, started.title), status: "running" }, ...this.tasks];
      });
      await api.onTaskStream((message) => {
        const id = message.type === "event" ? message.event.task_id : message.task_id;
        if (adopted.has(id)) this.apply(id, message);
      });
    } catch {
      // Optional.
    }
    try {
      await this.followChannelTasks();
    } catch {
      // Optional: channels are an extra way in.
    }
    try {
      this.workspaces = await api.workspaceList();
      this.activeWorkspaceId ??= this.workspaces[0]?.id ?? null;
    } catch (e) {
      this.error = String(e);
    }
  }

  async openWorkspace(path: string): Promise<boolean> {
    try {
      const ws = await api.workspaceOpen(path);
      this.workspaces = await api.workspaceList();
      this.activeWorkspaceId = ws.id;
      this.error = null;
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  selectWorkspace(id: string): void {
    this.activeWorkspaceId = id;
  }
  selectTask(id: string): void {
    this.activeTaskId = id;
  }

  cycleMode(): void {
    this.mode = cycleMode(this.mode);
  }
  setMode(mode: ModeName): void {
    this.mode = mode;
  }

  /** Agent profile for new tasks ("" = the mode's default). */
  profile = $state<string>("");
  profiles = $state<{ name: string; description: string }[]>([]);

  async loadProfiles(): Promise<void> {
    const ws = this.activeWorkspace;
    if (!ws) {
      this.profiles = [];
      return;
    }
    try {
      const list = await api.agentProfiles(ws.id);
      this.profiles = list.profiles;
      if (list.warnings.length) this.error = `Agent profiles: ${list.warnings.join("; ")}`;
    } catch (e) {
      this.error = String(e);
    }
  }

  async undoStep(taskId: string): Promise<void> {
    try {
      const outcome = await api.taskUndoStep(taskId);
      const files = outcome.restored.length ? outcome.restored.join(", ") : "no file changes";
      this.tasks = this.tasks.map((t) =>
        t.id === taskId
          ? {
              ...t,
              snapshots: outcome.remaining,
              items: [
                ...t.items,
                { kind: "notice", id: `undo:${outcome.seq}`, tone: "info", text: `Undid ${outcome.tool} (${outcome.call_id}): ${files}` },
              ],
            }
          : t,
      );
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  async startTask(prompt: string): Promise<void> {
    const ws = this.activeWorkspace;
    if (!ws || !prompt.trim()) return;
    const id = await this.trackTask(prompt, (deliver) =>
      api.taskStart(ws.id, prompt, this.mode, deliver, this.profile || undefined),
    );
    if (id) this.activeTaskId = id;
  }

  /**
   * Show a task another screen started (Design Studio writes, conversions,
   * "Implement this"): `start` gets the message callback and returns the id.
   */
  async trackTask(title: string, start: (deliver: (m: StreamMessage) => void) => Promise<string>): Promise<string | null> {
    const buffered: StreamMessage[] = [];
    let taskId: string | null = null;
    const deliver = (message: StreamMessage) => {
      if (taskId === null) buffered.push(message);
      else this.apply(taskId, message);
    };
    try {
      const id = await start(deliver);
      this.tasks = [{ ...newTask(id, title.slice(0, 60)), status: "running" }, ...this.tasks];
      taskId = id;
      for (const m of buffered) this.apply(id, m);
      this.error = null;
      return id;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  /** Tasks started from a chat channel (not from this window). */
  private channelTasks = new Set<string>();

  /** Show channel tasks: they arrive as broadcast `task-started` (with an
   * origin) and `task-stream` events instead of a direct Channel. */
  private async followChannelTasks(): Promise<void> {
    await api.onTaskStarted((started) => {
      if (!started.origin || this.channelTasks.has(started.taskId)) return;
      this.channelTasks.add(started.taskId);
      const task = { ...newTask(started.taskId, started.title), status: "running" as const, origin: started.origin };
      this.tasks = [task, ...this.tasks];
      this.activeTaskId ??= started.taskId;
    });
    await api.onTaskStream((message) => {
      const id = messageTaskId(message);
      if (this.channelTasks.has(id)) this.apply(id, message);
    });
  }

  private apply(taskId: string, message: StreamMessage): void {
    this.tasks = this.tasks.map((t) => (t.id === taskId ? applyMessage(t, message) : t));
  }

  async respond(taskId: string, approvalId: string, choice: ApprovalChoice): Promise<void> {
    try {
      await api.approvalRespond(taskId, approvalId, choice);
      this.tasks = this.tasks.map((t) => (t.id === taskId ? resolveApproval(t, approvalId, choice) : t));
    } catch (e) {
      this.error = String(e);
    }
  }

  async review(taskId: string, action: DiffAction): Promise<void> {
    try {
      const outcome = await api.reviewAction(taskId, action);
      const label =
        outcome.action === "merge"
          ? `merged${outcome.commit ? ` (${outcome.commit.slice(0, 8)})` : ""}`
          : outcome.action === "discard"
            ? "discarded"
            : "kept for later";
      this.tasks = this.tasks.map((t) => (t.id === taskId ? settleReview(t, label) : t));
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  async cancel(taskId: string): Promise<void> {
    await api.taskCancel(taskId);
  }
}

export const app = new AppStore();
