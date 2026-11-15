import assert from "node:assert/strict";
import test from "node:test";
import { rovingIndex } from "../src/lib/ds/keyboard.ts";
import { compareValues, nextSort, sortRows, type TableColumn } from "../src/lib/ds/table.ts";
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

type Row = { name: string; n: number | null; ts: string };
const rows: Row[] = [
  { name: "b10", n: 5, ts: "2026-02-01" },
  { name: "b2", n: null, ts: "2026-01-01" },
  { name: "a", n: 12, ts: "2026-03-01" },
  { name: "c", n: 5, ts: "2026-04-01" },
];
const col = (over: Partial<TableColumn<Row>>): TableColumn<Row> => ({ key: "k", label: "K", ...over });

test("sortRows sorts numbers numerically, strings naturally, and does not mutate", () => {
  const byN = sortRows(rows, col({ value: (r) => r.n }), "asc");
  assert.deepEqual(byN.map((r) => r.name), ["b10", "c", "a", "b2"]);
  const byName = sortRows(rows, col({ value: (r) => r.name }), "asc");
  assert.deepEqual(byName.map((r) => r.name), ["a", "b2", "b10", "c"]);
  assert.equal(rows[0]?.name, "b10");
});

test("sortRows keeps empty values last in both directions and ties stable", () => {
  const desc = sortRows(rows, col({ value: (r) => r.n }), "desc");
  assert.deepEqual(desc.map((r) => r.name), ["a", "b10", "c", "b2"]);
});

test("sortRows prefers sortValue and leaves order alone without a getter", () => {
  const formatted = col({ value: (r) => r.ts.slice(5), sortValue: (r) => r.ts });
  assert.deepEqual(sortRows(rows, formatted, "desc").map((r) => r.name), ["c", "a", "b10", "b2"]);
  assert.deepEqual(sortRows(rows, col({}), "asc"), rows);
  assert.deepEqual(sortRows(rows, undefined, "asc"), rows);
});

test("compareValues orders empties after values", () => {
  assert.ok(compareValues(1, undefined) < 0);
  assert.ok(compareValues("", "a") > 0);
  assert.equal(compareValues(null, undefined), 0);
});

test("nextSort starts a new column ascending and flips the same one", () => {
  assert.deepEqual(nextSort(undefined, "asc", "cost"), { key: "cost", dir: "asc" });
  assert.deepEqual(nextSort("cost", "asc", "cost"), { key: "cost", dir: "desc" });
  assert.deepEqual(nextSort("cost", "desc", "cost"), { key: "cost", dir: "asc" });
  assert.deepEqual(nextSort("cost", "desc", "name"), { key: "name", dir: "asc" });
});
