import assert from "node:assert/strict";
import test from "node:test";
import { cycleMode, EVERYDAY_MODES, MODE_LABELS, MODE_TONE } from "../src/lib/modes.ts";

test("cycles plan -> manual -> auto -> plan", () => {
  assert.equal(cycleMode("plan"), "manual");
  assert.equal(cycleMode("manual"), "auto");
  assert.equal(cycleMode("auto"), "plan");
});

test("bypass is never reachable by cycling", () => {
  assert.equal(EVERYDAY_MODES.includes("bypassPermissions"), false);
  let mode = cycleMode("plan");
  for (let i = 0; i < 9; i += 1) {
    assert.notEqual(mode, "bypassPermissions");
    mode = cycleMode(mode);
  }
  assert.equal(cycleMode("bypassPermissions"), "manual");
});

test("every mode has a label and a tone", () => {
  assert.deepEqual(Object.keys(MODE_LABELS).sort(), [
    "auto",
    "bypassPermissions",
    "manual",
    "plan",
  ]);
  assert.equal(MODE_LABELS.bypassPermissions, "Bypass");
  assert.equal(MODE_TONE.bypassPermissions, "danger");
});
