import assert from "node:assert/strict";
import test from "node:test";
import { diffStats, sideBySide, type DiffFile } from "../src/lib/diff.ts";

test("diffStats counts added and removed lines across files", () => {
  const files: DiffFile[] = [
    {
      path: "a.ts",
      status: "modified",
      hunks: [
        {
          oldStart: 1, oldLines: 2, newStart: 1, newLines: 3,
          lines: [
            { kind: "context", text: "x" },
            { kind: "remove", text: "y" },
            { kind: "add", text: "z" },
            { kind: "add", text: "w" },
          ],
        },
      ],
    },
  ];
  assert.deepEqual(diffStats(files), { added: 2, removed: 1 });
  assert.deepEqual(diffStats([]), { added: 0, removed: 0 });
});

test("sideBySide pairs removed and added runs and numbers both sides", () => {
  const rows = sideBySide({
    oldStart: 10,
    oldLines: 3,
    newStart: 10,
    newLines: 4,
    lines: [
      { kind: "context", text: "a" },
      { kind: "remove", text: "b" },
      { kind: "add", text: "B" },
      { kind: "add", text: "B2" },
      { kind: "context", text: "c" },
    ],
  });
  assert.equal(rows.length, 4);
  assert.deepEqual(rows[1], {
    left: { line: 11, text: "b", kind: "remove" },
    right: { line: 11, text: "B", kind: "add" },
  });
  assert.equal(rows[2].left, undefined);
  assert.equal(rows[2].right?.line, 12);
  assert.deepEqual(rows[3].left?.line, 12);
  assert.deepEqual(rows[3].right?.line, 13);
});
