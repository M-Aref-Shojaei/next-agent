// Next Agent: island size from the measured notch (src/core/layout.ts islandSize,
// upstream's islandSize(nw:nh:)). Nothing of the compact island may land inside
// the notch rectangle, which is centred under the screen's top edge.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  COMPACT_EAR, COMPACT_PLAN_RIGHT_ALONE, COMPACT_PLAN_RIGHT_WITH_GRID, COMPACT_W, EXPANDED_W,
  NOTCH_H, NOTCH_W, compactEar, islandSize,
} from "../src/core/layout.ts";

// 16" MacBook Pro as measured here: 1728 pt wide, two 771.5 pt menu-bar halves.
const WIDE = { w: 185, h: 32 };
// A notch wider than the 184 pt the PC layout assumes, and taller.
const WIDER = { w: 200, h: 38 };

test("a notch wider than 184 gets an island that reaches out 80 pt each side of it", () => {
  assert.deepEqual(islandSize("compact", "overview", 0, WIDER), { w: 200 + 160, h: 38 });
  assert.deepEqual(islandSize("compact", "overview", 0, WIDE), { w: 185 + 160, h: 32 });
});

test("folded on a notch, the island is the notch's width and draws no height", () => {
  assert.deepEqual(islandSize("hidden", "overview", 0, WIDER), { w: 200, h: 0 });
});

test("no notch keeps today's sizes (external display, or a Mac without one)", () => {
  assert.deepEqual(islandSize("compact", "overview", 0, null), { w: COMPACT_W, h: NOTCH_H });
  assert.deepEqual(islandSize("compact", "overview"), { w: COMPACT_W, h: NOTCH_H });
  assert.deepEqual(islandSize("hidden", "overview", 0, null), { w: NOTCH_W, h: 0 });
  assert.equal(islandSize("expanded", "overview", 0, null).w, EXPANDED_W);
});

test("expanded is the same with or without a notch", () => {
  assert.deepEqual(islandSize("expanded", "overview", 0, WIDER), islandSize("expanded", "overview", 0, null));
});

test("the mini grid and the plan pill clear the notch on the right", () => {
  for (const notch of [WIDE, WIDER]) {
    for (const grid of [true, false]) {
      const ear = compactEar(true, grid);
      const { w } = islandSize("compact", "overview", 0, notch, ear);
      const notchRight = w / 2 + notch.w / 2;
      // The mini grid is 29 wide, centred 40 from the island's right edge.
      assert.ok(w - 40 - 14.5 >= notchRight, `grid under the notch (${notch.w}, grid=${grid})`);
      // The plan pill is 58 wide at most, `right` px from the island's right edge.
      const right = grid ? COMPACT_PLAN_RIGHT_WITH_GRID : COMPACT_PLAN_RIGHT_ALONE;
      assert.ok(w - right - 58 >= notchRight, `plan pill under the notch (${notch.w}, grid=${grid})`);
    }
  }
});

test("without the plan pill the ear is upstream's 80", () => {
  assert.equal(compactEar(false, true), COMPACT_EAR);
  assert.equal(compactEar(false, false), COMPACT_EAR);
});
