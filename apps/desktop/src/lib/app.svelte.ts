// App-wide reactive state (Svelte 5 runes). Logic lives in pure modules
// (timeline.ts, modes.ts, shortcuts.ts); this class only wires them to the API.
import type { ModeName } from "@next-agent/contracts";
import { api, type Workspace } from "./api.ts";
import { cycleMode } from "./modes.ts";
import {
  applyMessage,
  newTask,
  resolveApproval,
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
  /** Stubbed until the router health poller (Step 6) lands. */
  router = $state<RouterHealth>("unknown");
  /** Stubbed until worktree isolation (Step 4) lands. */
  branch = $state<string | null>(null);
  error = $state<string | null>(null);

  get activeWorkspace(): Workspace | undefined {
    return this.workspaces.find((w) => w.id === this.activeWorkspaceId);
  }
  get activeTask(): TaskState | undefined {
    return this.tasks.find((t) => t.id === this.activeTaskId);
  }
  get pendingCount(): number {
    return this.tasks.reduce((n, t) => n + t.approvals.filter((a) => a.choice === undefined).length, 0);
  }

  async init(): Promise<void> {
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

  async startTask(prompt: string): Promise<void> {
    const ws = this.activeWorkspace;
    if (!ws || !prompt.trim()) return;
    const buffered: StreamMessage[] = [];
    let taskId: string | null = null;
    const deliver = (message: StreamMessage) => {
      if (taskId === null) buffered.push(message);
      else this.apply(taskId, message);
    };
    try {
      const id = await api.taskStart(ws.id, prompt, this.mode, deliver);
      this.tasks = [{ ...newTask(id, prompt.slice(0, 60)), status: "running" }, ...this.tasks];
      this.activeTaskId = id;
      taskId = id;
      for (const m of buffered) this.apply(id, m);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  private apply(taskId: string, message: StreamMessage): void {
    this.tasks = this.tasks.map((t) => (t.id === taskId ? applyMessage(t, message) : t));
  }

  async respond(taskId: string, approvalId: string, choice: ApprovalChoice): Promise<void> {
    try {
      await api.approvalRespond(approvalId, choice);
      this.tasks = this.tasks.map((t) => (t.id === taskId ? resolveApproval(t, approvalId, choice) : t));
    } catch (e) {
      this.error = String(e);
    }
  }

  async cancel(taskId: string): Promise<void> {
    await api.taskCancel(taskId);
  }
}

export const app = new AppStore();
