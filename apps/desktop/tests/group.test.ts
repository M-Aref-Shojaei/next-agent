import assert from "node:assert/strict";
import { test } from "node:test";
import { groupByPage } from "../src/lib/notes/group.ts";
import type { Backlink } from "../src/lib/notes/types.ts";

const row = (page: string, blockId: number): Backlink => ({ page, journalDay: null, blockId, uuid: null, content: `c${blockId}`, breadcrumb: [] });

test("groups rows by page in first-seen order", () => {
  const groups = groupByPage([row("B", 1), row("A", 2), row("C", 3)]);
  assert.deepEqual(groups.map(([page]) => page), ["B", "A", "C"]);
});

test("no rows, no groups", () => {
  assert.deepEqual(groupByPage([]), []);
});

test("several rows of one page stay together, in row order", () => {
  const groups = groupByPage([row("A", 1), row("B", 2), row("A", 3), row("B", 4), row("A", 5)]);
  assert.deepEqual(groups.map(([page, items]) => [page, items.map((r) => r.blockId)]), [
    ["A", [1, 3, 5]],
    ["B", [2, 4]],
  ]);
});
