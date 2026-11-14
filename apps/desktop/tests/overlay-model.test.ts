import assert from "node:assert/strict";
import test from "node:test";
import {
  APPROVAL_TIMEOUT_MS,
  activeTask,
  emptyModel,
  nextTimeout,
  runningCount,
  ingest,
  ingestStarted,
  markAnswered,
  pendingQueue,
  wantsAttention,
} from "../src/lib/overlay-model.ts";
import type { StreamMessage } from "../src/lib/timeline.ts";

let seq = 0;
function ev(task: string, kind: string, data: object): StreamMessage {
  seq += 1;
  return {
    type: "event",
    event: { v: 1, id: `${task}:${seq}`, task_id: task, seq, ts: "2026-01-01T00:00:00Z", kind, data },
  };
}
const ask = (task: string, id: string) => [
  ev(task, "tool_requested", { call_id: `c-${id}`, tool: "write_file" }),
  ev(task, "approval_requested", { approval_id: id, call_id: `c-${id}`, summary: `write ${id}` }),
];
const run = (...messages: StreamMessage[]) => messages.reduce(ingest, emptyModel());

test("approvals queue first-in-first-out across tasks", () => {
  const m = run(...ask("a", "a1"), ...ask("b", "b1"), ...ask("a", "a2"));
  assert.deepEqual(
    pendingQueue(m).visible.map((i) => i.approvalId),
    ["a1", "b1", "a2"],
  );
  assert.ok(wantsAttention(m));
});

test("only five are visible; the rest are counted", () => {
  const m = run(...[1, 2, 3, 4, 5, 6, 7].flatMap((n) => ask("a", `x${n}`)));
  const q = pendingQueue(m);
  assert.equal(q.visible.length, 5);
  assert.equal(q.hidden, 2);
  assert.equal(q.total, 7);
  assert.equal(q.visible[0].approvalId, "x1");
});

test("answering or resolving removes it from the queue", () => {
  let m = run(...ask("a", "a1"), ...ask("a", "a2"));
  m = markAnswered(m, "a", "a1", "allow");
  assert.deepEqual(pendingQueue(m).visible.map((i) => i.approvalId), ["a2"]);
  m = ingest(m, ev("a", "approval_resolved", { approval_id: "a2", choice: "deny" }));
  assert.equal(pendingQueue(m).total, 0);
  assert.equal(wantsAttention(m), false);
});

test("a finished task drops its pending approvals", () => {
  const m = run(...ask("a", "a1"), ev("a", "task_cancelled", {}), ev("b", "task_started", { mode: "manual" }));
  // `task_cancelled` may not be a known kind; force the status the reducer uses.
  const done = { ...m, tasks: m.tasks.map((t) => (t.id === "a" ? { ...t, status: "cancelled" as const } : t)) };
  assert.equal(pendingQueue(done).total, 0);
});

test("duplicate approval messages do not duplicate the queue", () => {
  const dup = ev("a", "approval_requested", { approval_id: "a1", call_id: "c", summary: "s" });
  const m = run(dup, dup);
  assert.equal(pendingQueue(m).total, 1);
});

test("active task prefers the running one; title and mode come from the start notice", () => {
  let m = run(ev("old", "task_started", { mode: "plan" }));
  m = ingestStarted(m, { taskId: "new", title: "Fix the build", mode: "auto" });
  assert.equal(activeTask(m)?.id, "new");
  assert.equal(activeTask(m)?.title, "Fix the build");
  assert.equal(m.modes.new, "auto");
  assert.equal(m.modes.old, "plan");
  m = ingestStarted(m, { taskId: "old", title: "Earlier", mode: "plan" });
  assert.equal(m.tasks.find((t) => t.id === "old")?.title, "Earlier");
});

test("events may arrive before the started notice", () => {
  let m = run(ev("t", "task_started", { mode: "manual" }));
  m = ingestStarted(m, { taskId: "t", title: "Late title", mode: "manual" });
  assert.equal(m.tasks.length, 1);
  assert.equal(activeTask(m)?.title, "Late title");
});

test("a request leaves the island after Coucou's 115 s wait but is still counted", () => {
  const t0 = 1_000_000;
  let m = emptyModel();
  for (const msg of ask("a", "a1")) m = ingest(m, msg, t0);
  for (const msg of ask("a", "a2")) m = ingest(m, msg, t0 + 60_000);
  assert.equal(nextTimeout(m, t0), t0 + APPROVAL_TIMEOUT_MS);
  const later = pendingQueue(m, undefined, t0 + APPROVAL_TIMEOUT_MS);
  assert.deepEqual(later.visible.map((i) => i.approvalId), ["a2"]);
  assert.equal(later.expired, 1);
  assert.equal(later.total, 2);
  assert.equal(wantsAttention(m, t0 + 60_000 + APPROVAL_TIMEOUT_MS), false);
});

test("running tasks are counted", () => {
  let m = run(ev("a", "task_started", { mode: "manual" }));
  m = ingestStarted(m, { taskId: "b", title: "B", mode: "auto" });
  assert.equal(runningCount(m), 2);
});
