// Pure timeline reducer: Tauri Channel messages in, render-ready state out.
// Kept free of Svelte so it runs under `node --test`.
import type { EventEnvelope } from "@next-agent/contracts";

export type ToolStatus = "running" | "ok" | "failed" | "denied";

export interface MessageItem {
  kind: "message";
  id: string;
  role: "agent" | "plan";
  text: string;
}
export interface ToolItem {
  kind: "tool";
  id: string;
  callId: string;
  tool: string;
  status: ToolStatus;
  decision?: string;
  reason?: string;
  output?: string;
}
export interface NoticeItem {
  kind: "notice";
  id: string;
  tone: "info" | "ok" | "error";
  text: string;
}
export type TimelineItem = MessageItem | ToolItem | NoticeItem;

export type ApprovalChoice = "allow" | "deny" | "always";

export interface ApprovalItem {
  approvalId: string;
  callId: string;
  tool: string;
  summary: string;
  reason: string;
  /** Set once answered. */
  choice?: ApprovalChoice;
}

export type TaskStatus = "pending" | "running" | "completed" | "failed" | "cancelled";

export interface TaskState {
  id: string;
  title: string;
  status: TaskStatus;
  items: TimelineItem[];
  approvals: ApprovalItem[];
  /** Highest applied `seq`; events at or below it are duplicates. */
  lastSeq: number;
  /** Tool output that arrived before its `tool_requested` event. */
  stashedOutput: Record<string, string>;
}

export type StreamMessage =
  | { type: "event"; event: EventEnvelope }
  | {
      type: "approval";
      task_id: string;
      approval_id: string;
      call_id: string;
      tool: string;
      summary: string;
      reason: string;
    }
  | { type: "tool_output"; task_id: string; call_id: string; ok: boolean; output: string };

export function newTask(id: string, title: string): TaskState {
  return {
    id,
    title,
    status: "pending",
    items: [],
    approvals: [],
    lastSeq: -1,
    stashedOutput: {},
  };
}

export function pendingApprovals(task: TaskState): ApprovalItem[] {
  return task.approvals.filter((a) => a.choice === undefined);
}

export function resolveApproval(
  task: TaskState,
  approvalId: string,
  choice: ApprovalChoice,
): TaskState {
  return {
    ...task,
    approvals: task.approvals.map((a) => (a.approvalId === approvalId ? { ...a, choice } : a)),
  };
}

function field(data: unknown, key: string): string {
  if (typeof data !== "object" || data === null) return "";
  const value = (data as Record<string, unknown>)[key];
  return typeof value === "string" ? value : "";
}

function updateTool(items: TimelineItem[], callId: string, patch: Partial<ToolItem>): TimelineItem[] {
  return items.map((item) =>
    item.kind === "tool" && item.callId === callId ? { ...item, ...patch } : item,
  );
}

/** Apply one stream message, returning a new state. Pure. */
export function applyMessage(task: TaskState, message: StreamMessage): TaskState {
  if (message.type === "approval") {
    if (task.approvals.some((a) => a.approvalId === message.approval_id)) return task;
    return {
      ...task,
      approvals: [
        ...task.approvals,
        {
          approvalId: message.approval_id,
          callId: message.call_id,
          tool: message.tool,
          summary: message.summary,
          reason: message.reason,
        },
      ],
    };
  }
  if (message.type === "tool_output") {
    const exists = task.items.some((i) => i.kind === "tool" && i.callId === message.call_id);
    if (!exists) {
      return {
        ...task,
        stashedOutput: { ...task.stashedOutput, [message.call_id]: message.output },
      };
    }
    return { ...task, items: updateTool(task.items, message.call_id, { output: message.output }) };
  }

  const { event } = message;
  if (event.seq <= task.lastSeq) return task;
  const next: TaskState = { ...task, lastSeq: event.seq };
  const data = event.data;
  const id = event.id;

  switch (event.kind) {
    case "task_started":
      return {
        ...next,
        status: "running",
        items: [
          ...next.items,
          {
            kind: "notice",
            id,
            tone: "info",
            text: `Started in ${field(data, "mode")} mode (${field(data, "executor")})`,
          },
        ],
      };
    case "message_delta": {
      const last = next.items[next.items.length - 1];
      if (last?.kind === "message" && last.role === "agent") {
        const merged: MessageItem = { ...last, text: last.text + field(data, "text") };
        return { ...next, items: [...next.items.slice(0, -1), merged] };
      }
      return {
        ...next,
        items: [...next.items, { kind: "message", id, role: "agent", text: field(data, "text") }],
      };
    }
    case "message":
      return {
        ...next,
        items: [...next.items, { kind: "message", id, role: "agent", text: field(data, "text") }],
      };
    case "plan_proposed":
      return {
        ...next,
        items: [
          ...next.items,
          { kind: "message", id, role: "plan", text: field(data, "markdown") },
        ],
      };
    case "tool_requested": {
      const callId = field(data, "call_id");
      const output = next.stashedOutput[callId];
      const { [callId]: _used, ...rest } = next.stashedOutput;
      const tool: ToolItem = {
        kind: "tool",
        id,
        callId,
        tool: field(data, "tool"),
        status: "running",
        ...(output === undefined ? {} : { output }),
      };
      return { ...next, stashedOutput: rest, items: [...next.items, tool] };
    }
    case "permission_decided": {
      const decision = field(data, "decision");
      const blocked = decision === "deny" || decision === "ask";
      return {
        ...next,
        items: updateTool(next.items, field(data, "call_id"), {
          decision,
          reason: field(data, "reason"),
          ...(blocked ? { status: "denied" as const } : {}),
        }),
      };
    }
    case "tool_finished": {
      const ok = typeof data === "object" && data !== null && (data as { ok?: unknown }).ok === true;
      const callId = field(data, "call_id");
      const current = next.items.find((i) => i.kind === "tool" && i.callId === callId);
      if (current?.kind === "tool" && current.status === "denied") return next;
      return { ...next, items: updateTool(next.items, callId, { status: ok ? "ok" : "failed" }) };
    }
    case "task_completed":
      return {
        ...next,
        status: "completed",
        items: [
          ...next.items,
          { kind: "notice", id, tone: "ok", text: `Completed: ${field(data, "summary")}` },
        ],
      };
    case "task_failed":
      return {
        ...next,
        status: "failed",
        items: [
          ...next.items,
          { kind: "notice", id, tone: "error", text: `Failed: ${field(data, "error")}` },
        ],
      };
    case "task_cancelled":
      return {
        ...next,
        status: "cancelled",
        items: [...next.items, { kind: "notice", id, tone: "info", text: "Cancelled" }],
      };
    default:
      return next;
  }
}
