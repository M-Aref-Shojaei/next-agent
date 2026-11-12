export const CONTRACT_VERSION = 1;

export type ModeName = "plan" | "manual" | "auto" | "bypassPermissions";

export interface EventEnvelope<T = unknown> {
  v: typeof CONTRACT_VERSION;
  id: string;
  task_id: string;
  seq: number;
  ts: string;
  kind: string;
  data: T;
}

/** Orders/dedupes events by per-task sequence number. */
export function compareEvents(a: EventEnvelope, b: EventEnvelope): number {
  if (a.task_id !== b.task_id) return a.task_id < b.task_id ? -1 : 1;
  return a.seq - b.seq;
}

export function isEnvelope(value: unknown): value is EventEnvelope {
  if (typeof value !== "object" || value === null) return false;
  const e = value as Record<string, unknown>;
  return (
    e.v === CONTRACT_VERSION &&
    typeof e.id === "string" &&
    typeof e.task_id === "string" &&
    typeof e.seq === "number" &&
    Number.isInteger(e.seq) &&
    e.seq >= 0 &&
    typeof e.ts === "string" &&
    typeof e.kind === "string"
  );
}
