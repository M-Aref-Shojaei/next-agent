import assert from "node:assert/strict";
import test from "node:test";
import { cycleMode } from "../src/lib/modes.ts";
import { bypassConfirmed, shouldCycleMode, type KeyLike } from "../src/lib/shortcuts.ts";

const key = (over: Partial<KeyLike>): KeyLike => ({
  key: "Tab", shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...over,
});

test("Shift+Tab cycles only while the chat input is focused (Q6)", () => {
  assert.equal(shouldCycleMode(key({ shiftKey: true }), true), true);
  assert.equal(shouldCycleMode(key({ shiftKey: true }), false), false);
});

test("plain Tab never cycles", () => {
  assert.equal(shouldCycleMode(key({}), true), false);
});

test("Ctrl/Cmd+Shift+M is a global chord", () => {
  assert.equal(shouldCycleMode(key({ key: "M", shiftKey: true, ctrlKey: true }), false), true);
  assert.equal(shouldCycleMode(key({ key: "m", shiftKey: true, metaKey: true }), false), true);
  assert.equal(shouldCycleMode(key({ key: "m", shiftKey: true }), false), false);
});

test("repeated cycling from every start never lands on bypass", () => {
  for (const start of ["plan", "manual", "auto", "bypassPermissions"] as const) {
    let mode = start;
    for (let i = 0; i < 6; i += 1) {
      mode = cycleMode(mode);
      assert.notEqual(mode, "bypassPermissions");
    }
  }
});

test("bypass needs the exact workspace name", () => {
  assert.equal(bypassConfirmed("proj", "proj"), true);
  assert.equal(bypassConfirmed("  proj ", "proj"), true);
  assert.equal(bypassConfirmed("Proj", "proj"), false);
  assert.equal(bypassConfirmed("", ""), false);
});
