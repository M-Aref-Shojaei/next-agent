// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/ApprovalQueue.swift and the request timeout in NotchBuddy/Sources/App/HookServer.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Pure model behind the overlay window: the task stream in, a FIFO approval
// queue and the active task out. Free of Svelte so it runs under `node --test`.
//
// Coucou's queue: first in, first out; at most five pending; each request
// waits 115 s; requests are cleared per session when it stops. Ours: the
// approvals live in the app, so a sixth one, or one past its timeout, is not
// lost: it leaves the island and is counted as waiting in Next Agent (Coucou
// hands it back to the terminal). A task that ends clears its approvals.
import {
  applyMessage,
  newTask,
  resolveApproval,
  type ApprovalChoice,
  type StreamMessage,
  type TaskState,
} from "./timeline.ts";

/** At most this many approvals are queued on the island; the rest are counted. */
export const MAX_VISIBLE = 5;
/** How long a request stays on the island (Coucou: 115 s per request). */
export const APPROVAL_TIMEOUT_MS = 115_000;

export interface OverlayModel {
  /** Newest first. */
  tasks: TaskState[];
  modes: Record<string, string>;
  /** Arrival order of each approval, so the queue is FIFO across tasks. */
  arrivals: Record<string, number>;
  /** When each approval arrived (ms, `Date.now()` clock), for the timeout. */
  received: Record<string, number>;
  counter: number;
}

export interface QueueItem {
  taskId: string;
  taskTitle: string;
  approvalId: string;
  tool: string;
  summary: string;
  reason: string;
}

export interface Queue {
  /** Oldest first; `visible[0]` is the card the island shows. */
  visible: QueueItem[];
  /** Waiting beyond the first five. */
  hidden: number;
  /** Past the island's timeout (still waiting in the app). */
  expired: number;
  /** Everything still waiting, on the island or not. */
  total: number;
}

export function emptyModel(): OverlayModel {
  return { tasks: [], modes: {}, arrivals: {}, received: {}, counter: 0 };
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);

function pendingIds(task: TaskState | undefined): string[] {
  return task ? task.approvals.filter((a) => a.choice === undefined).map((a) => a.approvalId) : [];
}

/** Replace one task and stamp arrival order and time for approvals that just appeared. */
function put(model: OverlayModel, before: TaskState | undefined, after: TaskState, now: number): OverlayModel {
  const known = new Set(pendingIds(before));
  const arrivals = { ...model.arrivals };
  const received = { ...model.received };
  let counter = model.counter;
  for (const id of pendingIds(after)) {
    if (!known.has(id) && arrivals[id] === undefined) {
      arrivals[id] = counter++;
      received[id] = now;
    }
  }
  const tasks = before ? model.tasks.map((t) => (t.id === after.id ? after : t)) : [after, ...model.tasks];
  return { ...model, tasks, arrivals, received, counter };
}

function taskIdOf(message: StreamMessage): string {
  return message.type === "event" ? message.event.task_id : message.task_id;
}

/** A task was started in the main window (title comes from the prompt). */
export function ingestStarted(
  model: OverlayModel,
  started: { taskId: string; title: string; mode: string },
  now = Date.now(),
): OverlayModel {
  const before = model.tasks.find((t) => t.id === started.taskId);
  const modes = { ...model.modes, [started.taskId]: started.mode };
  if (before) return put({ ...model, modes }, before, { ...before, title: started.title }, now);
  return put({ ...model, modes }, undefined, { ...newTask(started.taskId, started.title), status: "running" }, now);
}

/** Apply one broadcast stream message. Events may precede `ingestStarted`. */
export function ingest(model: OverlayModel, message: StreamMessage, now = Date.now()): OverlayModel {
  const id = taskIdOf(message);
  const before = model.tasks.find((t) => t.id === id);
  const after = applyMessage(before ?? newTask(id, "Task"), message);
  let next = put(model, before, after, now);
  if (message.type === "event" && message.event.kind === "task_started") {
    const mode = (message.event.data as { mode?: unknown }).mode;
    if (typeof mode === "string") next = { ...next, modes: { ...next.modes, [id]: mode } };
  }
  return next;
}

/** Record an answer given in the overlay (the event stream confirms it later). */
export function markAnswered(
  model: OverlayModel,
  taskId: string,
  approvalId: string,
  choice: ApprovalChoice,
): OverlayModel {
  const before = model.tasks.find((t) => t.id === taskId);
  if (!before) return model;
  return put(model, before, resolveApproval(before, approvalId, choice), Date.now());
}

/**
 * Pending approvals of live tasks, oldest first, at most `max` on the island.
 * With `now`, requests older than `APPROVAL_TIMEOUT_MS` leave the island.
 */
export function pendingQueue(model: OverlayModel, max = MAX_VISIBLE, now?: number): Queue {
  const items: { order: number; item: QueueItem }[] = [];
  let expired = 0;
  for (const task of model.tasks) {
    if (TERMINAL.has(task.status)) continue;
    for (const a of task.approvals) {
      if (a.choice !== undefined) continue;
      const at = model.received[a.approvalId];
      if (now !== undefined && at !== undefined && now - at >= APPROVAL_TIMEOUT_MS) {
        expired += 1;
        continue;
      }
      items.push({
        order: model.arrivals[a.approvalId] ?? Number.MAX_SAFE_INTEGER,
        item: {
          taskId: task.id,
          taskTitle: task.title,
          approvalId: a.approvalId,
          tool: a.tool,
          summary: a.summary,
          reason: a.reason,
        },
      });
    }
  }
  items.sort((a, b) => a.order - b.order);
  const all = items.map((i) => i.item);
  return {
    visible: all.slice(0, max),
    hidden: Math.max(0, all.length - max),
    expired,
    total: all.length + expired,
  };
}

/** When the next request on the island times out (ms), or `null`. */
export function nextTimeout(model: OverlayModel, now: number): number | null {
  let soonest: number | null = null;
  for (const item of pendingQueue(model, Number.MAX_SAFE_INTEGER, now).visible) {
    const at = model.received[item.approvalId];
    if (at === undefined) continue;
    const due = at + APPROVAL_TIMEOUT_MS;
    if (soonest === null || due < soonest) soonest = due;
  }
  return soonest;
}

/** Tasks still running: they keep the compact island on screen. */
export function runningCount(model: OverlayModel): number {
  return model.tasks.filter((t) => t.status === "running" || t.status === "pending").length;
}

/** The running task, else the newest one. */
export function activeTask(model: OverlayModel): TaskState | undefined {
  return model.tasks.find((t) => t.status === "running") ?? model.tasks[0];
}

/** Whether the island should stay open on its own (requests on the island). */
export function wantsAttention(model: OverlayModel, now?: number): boolean {
  return pendingQueue(model, MAX_VISIBLE, now).visible.length > 0;
}
