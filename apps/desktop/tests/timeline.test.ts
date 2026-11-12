import assert from "node:assert/strict";
import test from "node:test";
import {
  applyMessage,
  newTask,
  pendingApprovals,
  resolveApproval,
  settleReview,
  type StreamMessage,
  type TaskState,
} from "../src/lib/timeline.ts";

function ev(seq: number, kind: string, data: object): StreamMessage {
  return {
    type: "event",
    event: { v: 1, id: `t:${seq}`, task_id: "t", seq, ts: "2026-01-01T00:00:00Z", kind, data },
  };
}
const run = (task: TaskState, ...messages: StreamMessage[]) => messages.reduce(applyMessage, task);

test("tool call lifecycle builds one ordered tool card", () => {
  const task = run(
    newTask("t", "demo"),
    ev(0, "task_started", { mode: "auto", executor: "mock" }),
    ev(1, "tool_requested", { call_id: "c1", tool: "read_file" }),
    ev(2, "permission_decided", { call_id: "c1", decision: "allow", reason: "inside sandbox" }),
    { type: "tool_output", task_id: "t", call_id: "c1", ok: true, output: "hello" },
    ev(3, "tool_finished", { call_id: "c1", ok: true }),
    ev(4, "task_completed", { summary: "done" }),
  );
  assert.equal(task.status, "completed");
  const tools = task.items.filter((i) => i.kind === "tool");
  assert.equal(tools.length, 1);
  assert.deepEqual(
    { s: tools[0].status, o: tools[0].output, d: tools[0].decision },
    { s: "ok", o: "hello", d: "allow" },
  );
});

test("duplicate and out-of-order seqs are ignored", () => {
  const a = run(newTask("t", "x"), ev(0, "message", { text: "one" }), ev(1, "message", { text: "two" }));
  const b = run(a, ev(1, "message", { text: "two again" }), ev(0, "message", { text: "zero" }));
  assert.equal(b.items.length, 2);
  assert.equal(b.lastSeq, 1);
});

test("output that arrives before tool_requested is stashed then attached", () => {
  const task = run(
    newTask("t", "x"),
    { type: "tool_output", task_id: "t", call_id: "c9", ok: true, output: "early" },
    ev(0, "tool_requested", { call_id: "c9", tool: "run_command" }),
  );
  const tool = task.items[0];
  assert.equal(tool.kind === "tool" && tool.output, "early");
  assert.deepEqual(task.stashedOutput, {});
});

test("a denied call stays denied after tool_finished", () => {
  const task = run(
    newTask("t", "x"),
    ev(0, "tool_requested", { call_id: "c1", tool: "write_file" }),
    ev(1, "permission_decided", { call_id: "c1", decision: "deny", reason: "user declined" }),
    ev(2, "tool_finished", { call_id: "c1", ok: false }),
  );
  const tool = task.items[0];
  assert.equal(tool.kind === "tool" && tool.status, "denied");
});

test("approvals are pending until resolved, and never duplicated", () => {
  const approval: StreamMessage = {
    type: "approval",
    task_id: "t",
    approval_id: "t:1",
    call_id: "c1",
    tool: "write_file",
    summary: "NOTES.md",
    reason: "manual mode",
  };
  let task = run(newTask("t", "x"), approval, approval);
  assert.equal(pendingApprovals(task).length, 1);
  task = resolveApproval(task, "t:1", "always");
  assert.equal(pendingApprovals(task).length, 0);
  assert.equal(task.approvals[0].choice, "always");
});

test("message deltas coalesce into one agent message", () => {
  const task = run(
    newTask("t", "x"),
    ev(0, "message_delta", { text: "Hel" }),
    ev(1, "message_delta", { text: "lo" }),
  );
  assert.equal(task.items.length, 1);
  assert.equal(task.items[0].kind === "message" && task.items[0].text, "Hello");
});

test("failure and cancel set terminal status", () => {
  assert.equal(run(newTask("t", "x"), ev(0, "task_failed", { error: "boom" })).status, "failed");
  assert.equal(run(newTask("t", "x"), ev(0, "task_cancelled", {})).status, "cancelled");
});

test("approvals come from events: ask waits, deny marks the tool denied", () => {
  const task = run(
    newTask("t", "x"),
    ev(0, "tool_requested", { call_id: "c1", tool: "write_file", args: {} }),
    ev(1, "permission_decided", { call_id: "c1", decision: "ask", reason: "mutating tool in manual mode" }),
  );
  const waiting = task.items[0];
  assert.equal(waiting.kind === "tool" && waiting.status, "waiting");
  const asked = run(task, ev(2, "approval_requested", { approval_id: "apr_c1", call_id: "c1", summary: "NOTES.md", options: [] }));
  assert.equal(pendingApprovals(asked).length, 1);
  assert.equal(asked.approvals[0].tool, "write_file");
  assert.equal(asked.approvals[0].reason, "mutating tool in manual mode");

  const denied = run(
    asked,
    ev(3, "approval_resolved", { approval_id: "apr_c1", choice: "deny", by: "user" }),
    ev(4, "tool_finished", { call_id: "c1", ok: false, exit_code: null, summary: "denied by user" }),
  );
  assert.equal(pendingApprovals(denied).length, 0);
  const tool = denied.items[0];
  assert.equal(tool.kind === "tool" && tool.status, "denied");
});

test("allowed approval resumes; streamed output and summary fill the tool card", () => {
  const task = run(
    newTask("t", "x"),
    ev(0, "tool_requested", { call_id: "c1", tool: "run_command", args: {} }),
    ev(1, "permission_decided", { call_id: "c1", decision: "ask", reason: "r" }),
    ev(2, "approval_requested", { approval_id: "a1", call_id: "c1", summary: "ls", options: [] }),
    ev(3, "approval_resolved", { approval_id: "a1", choice: "always_for_workspace", by: "user" }),
    ev(4, "tool_output", { call_id: "c1", stream: "stdout", chunk: "a\n" }),
    ev(5, "tool_output", { call_id: "c1", stream: "stdout", chunk: "b\n" }),
    ev(6, "tool_finished", { call_id: "c1", ok: true, exit_code: 0, summary: "a\nb\n" }),
  );
  assert.equal(task.approvals[0].choice, "always");
  const tool = task.items[0];
  assert.equal(tool.kind === "tool" && tool.status, "ok");
  assert.equal(tool.kind === "tool" && tool.output, "a\nb\n");
});

test("worktree, review and documented messages land on the task", () => {
  const task = run(
    newTask("t", "x"),
    { type: "worktree", task_id: "t", branch: "next-agent/auto-1", root: "/w" },
    { type: "review", task_id: "t", branch: "next-agent/auto-1", files: [{ path: "a.md", status: "added", hunks: [] }] },
    { type: "documented", task_id: "t", path: "/vault/x.md" },
  );
  assert.equal(task.branch, "next-agent/auto-1");
  assert.equal(task.review?.files.length, 1);
  assert.equal(task.recordPath, "/vault/x.md");
  assert.equal(settleReview(task, "merged").review?.outcome, "merged");
});
