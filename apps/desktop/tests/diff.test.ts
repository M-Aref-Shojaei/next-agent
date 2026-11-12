import assert from "node:assert/strict";
import test from "node:test";
import { diffStats, type DiffFile } from "../src/lib/diff.ts";

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
