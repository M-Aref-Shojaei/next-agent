// Pure model behind the overlay window: the task stream in, a FIFO approval
// queue and the active task out. Free of Svelte so it runs under `node --test`.
//
// Design adapted from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/ApprovalQueue.swift
// — first-in-first-out, at most five visible, cleared when the task ends.
import {
  applyMessage,
  newTask,
  resolveApproval,
  type ApprovalChoice,
  type StreamMessage,
  type TaskState,
} from "./timeline.ts";

/** At most this many approvals are shown; the rest are counted. */
export const MAX_VISIBLE = 5;

export interface OverlayModel {
  /** Newest first. */
  tasks: TaskState[];
  modes: Record<string, string>;
  /** Arrival order of each approval, so the queue is FIFO across tasks. */
  arrivals: Record<string, number>;
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
  visible: QueueItem[];
  hidden: number;
  total: number;
}

export function emptyModel(): OverlayModel {
  return { tasks: [], modes: {}, arrivals: {}, counter: 0 };
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);

function pendingIds(task: TaskState | undefined): string[] {
  return task ? task.approvals.filter((a) => a.choice === undefined).map((a) => a.approvalId) : [];
}

/** Replace one task and stamp arrival order for approvals that just appeared. */
function put(model: OverlayModel, before: TaskState | undefined, after: TaskState): OverlayModel {
  const known = new Set(pendingIds(before));
  const arrivals = { ...model.arrivals };
  let counter = model.counter;
  for (const id of pendingIds(after)) {
    if (!known.has(id) && arrivals[id] === undefined) arrivals[id] = counter++;
  }
  const tasks = before
    ? model.tasks.map((t) => (t.id === after.id ? after : t))
    : [after, ...model.tasks];
  return { ...model, tasks, arrivals, counter };
}

function taskIdOf(message: StreamMessage): string {
  return message.type === "event" ? message.event.task_id : message.task_id;
}

/** A task was started in the main window (title comes from the prompt). */
export function ingestStarted(
  model: OverlayModel,
  started: { taskId: string; title: string; mode: string },
): OverlayModel {
  const before = model.tasks.find((t) => t.id === started.taskId);
  const modes = { ...model.modes, [started.taskId]: started.mode };
  if (before) return put({ ...model, modes }, before, { ...before, title: started.title });
  return put({ ...model, modes }, undefined, {
    ...newTask(started.taskId, started.title),
    status: "running",
  });
}

/** Apply one broadcast stream message. Events may precede `ingestStarted`. */
export function ingest(model: OverlayModel, message: StreamMessage): OverlayModel {
  const id = taskIdOf(message);
  const before = model.tasks.find((t) => t.id === id);
  const after = applyMessage(before ?? newTask(id, "Task"), message);
  let next = put(model, before, after);
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
  return put(model, before, resolveApproval(before, approvalId, choice));
}

/** Pending approvals of live tasks, oldest first, at most `max` visible. */
export function pendingQueue(model: OverlayModel, max = MAX_VISIBLE): Queue {
  const items: { order: number; item: QueueItem }[] = [];
  for (const task of model.tasks) {
    if (TERMINAL.has(task.status)) continue;
    for (const a of task.approvals) {
      if (a.choice !== undefined) continue;
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
  return { visible: all.slice(0, max), hidden: Math.max(0, all.length - max), total: all.length };
}

/** The running task, else the newest one. */
export function activeTask(model: OverlayModel): TaskState | undefined {
  return model.tasks.find((t) => t.status === "running") ?? model.tasks[0];
}

/** Whether the panel should stay open on its own. */
export function wantsAttention(model: OverlayModel): boolean {
  return pendingQueue(model).total > 0;
}
