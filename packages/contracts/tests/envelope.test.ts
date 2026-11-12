import assert from "node:assert/strict";
import test from "node:test";
import { CONTRACT_VERSION, compareEvents, isEnvelope } from "../src/index.ts";

const base = {
  v: CONTRACT_VERSION,
  id: "evt_1",
  task_id: "tsk_1",
  seq: 1,
  ts: "2026-11-10T10:00:00Z",
  kind: "task_started",
  data: {},
};

test("accepts a valid envelope", () => {
  assert.equal(isEnvelope(base), true);
});

test("rejects wrong version, seq and missing fields", () => {
  assert.equal(isEnvelope({ ...base, v: 2 }), false);
  assert.equal(isEnvelope({ ...base, seq: 1.5 }), false);
  assert.equal(isEnvelope({ ...base, seq: -1 }), false);
  assert.equal(isEnvelope({ ...base, kind: undefined }), false);
  assert.equal(isEnvelope(null), false);
  assert.equal(isEnvelope("evt"), false);
});

test("sorts events by task then seq", () => {
  const second = { ...base, id: "evt_2", seq: 2 };
  const otherTask = { ...base, id: "evt_3", task_id: "tsk_0", seq: 7 };
  const sorted = [second, otherTask, base].sort(compareEvents).map((e) => e.id);
  assert.deepEqual(sorted, ["evt_3", "evt_1", "evt_2"]);
});

test("duplicate sequences are detectable after sorting", () => {
  const dup = { ...base, id: "evt_9" };
  const sorted = [base, dup].sort(compareEvents);
  assert.equal(sorted[0].seq, sorted[1].seq);
  assert.notEqual(sorted[0].id, sorted[1].id);
});
