import assert from "node:assert/strict";
import test from "node:test";
import { rovingIndex } from "../src/lib/ds/keyboard.ts";
import { formatKeys, pressedKey, recordPress, shortcutLabel, shortcutParts, type KeyPress } from "../src/lib/ds/shortcut.ts";

const press = (over: Partial<KeyPress>): KeyPress => ({
  key: "k", code: "KeyK", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...over,
});

test("a chord records in canonical modifier order", () => {
  assert.deepEqual(recordPress(press({ metaKey: true, shiftKey: true, ctrlKey: true })), { kind: "keys", keys: "Ctrl+Shift+Meta+K" });
  assert.equal(formatKeys({ Meta: true, Alt: true }, "Space"), "Alt+Meta+Space");
});

test("the physical key wins over the typed character", () => {
  // Alt+K on a Mac types "˚"; Shift+1 types "!".
  assert.equal(pressedKey({ key: "˚", code: "KeyK" }), "K");
  assert.equal(pressedKey({ key: "!", code: "Digit1" }), "1");
  assert.equal(pressedKey({ key: " ", code: "Space" }), "Space");
  assert.equal(pressedKey({ key: "ArrowUp", code: "ArrowUp" }), "Up");
  assert.equal(pressedKey({ key: "F12", code: "F12" }), "F12");
  assert.equal(pressedKey({ key: "Unidentified", code: "" }), null);
});

test("modifiers alone keep listening; bare Esc cancels; bare Backspace clears", () => {
  assert.deepEqual(recordPress(press({ key: "Shift", code: "ShiftLeft", shiftKey: true })), { kind: "pending" });
  assert.deepEqual(recordPress(press({ key: "Escape", code: "Escape" })), { kind: "cancel" });
  assert.deepEqual(recordPress(press({ key: "Backspace", code: "Backspace" })), { kind: "clear" });
  assert.deepEqual(recordPress(press({ key: "Backspace", code: "Backspace", ctrlKey: true })), { kind: "keys", keys: "Ctrl+Backspace" });
});

test("a plain key needs a modifier unless the field allows it", () => {
  assert.deepEqual(recordPress(press({})), { kind: "needsModifier" });
  assert.deepEqual(recordPress(press({ shiftKey: true })), { kind: "needsModifier" });
  assert.deepEqual(recordPress(press({}), { requireModifier: false }), { kind: "keys", keys: "K" });
  assert.deepEqual(recordPress(press({ key: "Dead", code: "", ctrlKey: true })), { kind: "unsupported" });
});

test("display parts follow the platform; the plus key survives", () => {
  assert.deepEqual(shortcutParts("Ctrl+Shift+Meta+K", "mac"), ["⌃", "⇧", "⌘", "K"]);
  assert.deepEqual(shortcutParts("Ctrl+Shift+Meta+K", "other"), ["Ctrl", "Shift", "Win", "K"]);
  assert.deepEqual(shortcutParts("Ctrl++", "other"), ["Ctrl", "+"]);
  assert.deepEqual(shortcutParts("", "mac"), []);
  assert.equal(shortcutLabel("Meta+Alt+Escape", "mac"), "Command+Option+Escape");
  assert.equal(shortcutLabel("Meta+Escape", "other"), "Win+Escape");
});

test("roving focus wraps, skips disabled items and supports Home/End", () => {
  const off = [false, true, false, false];
  assert.equal(rovingIndex("ArrowRight", 0, off), 2);
  assert.equal(rovingIndex("ArrowLeft", 2, off), 0);
  assert.equal(rovingIndex("ArrowLeft", 0, off), 3);
  assert.equal(rovingIndex("ArrowRight", 3, off), 0);
  assert.equal(rovingIndex("Home", 3, [true, false, false]), 1);
  assert.equal(rovingIndex("End", 0, [false, false, true]), 1);
  assert.equal(rovingIndex("ArrowDown", 0, off), null);
  assert.equal(rovingIndex("ArrowDown", 0, off, "vertical"), 2);
  assert.equal(rovingIndex("Enter", 0, off), null);
  assert.equal(rovingIndex("ArrowRight", 0, [true, true]), null);
});
