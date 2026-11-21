import assert from "node:assert/strict";
import test from "node:test";
import { sideBySide, diffStats } from "../src/lib/diff.ts";

test("sideBySide pairs context lines correctly", () => {
  const hunk = {
    oldStart: 10,
    oldLines: 5,
    newStart: 12,
    newLines: 5,
    lines: [
      { kind: "context" as const, text: "line a" },
      { kind: "remove" as const, text: "old line b" },
      { kind: "add" as const, text: "new line B" },
      { kind: "context" as const, text: "line c" },
    ],
  };

  const rows = sideBySide(hunk);
  assert.equal(rows.length, 3);

  // First row: context on both sides
  assert.ok(rows[0].left && rows[0].right);
  assert.equal(rows[0].left.text, "line a");
  assert.equal(rows[0].right.text, "line a");

  // Second row: removed vs added
  assert.ok(!rows[1].right || !rows[1].left); // depends on implementation
});

test("diffStats counts additions and removals", () => {
  const files = [
    {
      path: "a.txt",
      status: "modified" as const,
      hunks: [
        { oldStart: 1, oldLines: 3, newStart: 1, newLines: 4, lines: [{ kind: "context", text: "line 1" }, { kind: "remove", text: "old" }, { kind: "add", text: "new" }] },
      ],
    },
  ];

  const stats = diffStats(files);
  assert.equal(stats.added, 1);
  assert.equal(stats.removed, 1);
});

test("sideBySide handles pure additions and removals", () => {
  // Pure addition
  const addHunk = {
    oldStart: 5,
    oldLines: 0,
    newStart: 5,
    newLines: 2,
    lines: [{ kind: "add" as const, text: "+ line A" }, { kind: "add" as const, text: "+ line B" }],
  };

  const rows = sideBySide(addHunk);
  assert.equal(rows.length, 2);
  assert.ok(!rows[0].left && rows[0].right); // right only

  // Pure removal
  const remHunk = {
    oldStart: 5,
    oldLines: 2,
    newStart: 5,
    newLines: 0,
    lines: [{ kind: "remove" as const, text: "- line A" }, { kind: "remove" as const, text: "- line B" }],
  };

  const rowsRem = sideBySide(remHunk);
  assert.equal(rowsRem.length, 2);
  assert.ok(rowsRem[0].left && !rowsRem[0].right); // left only
});
