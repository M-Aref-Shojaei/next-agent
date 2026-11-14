import assert from "node:assert/strict";
import test from "node:test";
import { CLOSE_MS, Spring, Tracked, closeCurve } from "../src/lib/island/anim.ts";
import { IslandStateMachine, type FsmState, type Timers } from "../src/lib/island/fsm.ts";
import {
  botPosition,
  islandCorner,
  islandFillOpacity,
  islandRect,
  islandSize,
  islandTopRadius,
  miniGrid,
  type IslandInfo,
} from "../src/lib/island/layout.ts";
import { islandPath } from "../src/lib/island/shape.ts";

/** A fake clock for the FSM's DispatchWorkItem-style timers. */
class FakeTimers implements Timers {
  now = 0;
  private queue: { at: number; fn: () => void; id: number }[] = [];
  private next = 1;
  set(fn: () => void, ms: number) {
    const id = this.next++;
    this.queue.push({ at: this.now + ms, fn, id });
    return id;
  }
  clear(handle: unknown) {
    this.queue = this.queue.filter((t) => t.id !== handle);
  }
  advance(ms: number) {
    const end = this.now + ms;
    for (;;) {
      this.queue.sort((a, b) => a.at - b.at);
      const due = this.queue[0];
      if (!due || due.at > end) break;
      this.queue.shift();
      this.now = due.at;
      due.fn();
    }
    this.now = end;
  }
}

function machine() {
  const timers = new FakeTimers();
  const fsm = new IslandStateMachine(timers);
  const seen: [FsmState, FsmState][] = [];
  fsm.onTransition = (from, to) => seen.push([from, to]);
  return { timers, fsm, seen };
}

// ── State machine (IslandStateMachine.swift) ────────────────────────────────

test("defaults are Coucou's: hover only peeks, 15 s auto-close, 3 min absence, 0.6 s hover close", () => {
  const { fsm } = machine();
  assert.equal(fsm.openOnHover, false);
  assert.equal(fsm.homeToPetitDelay, 15);
  assert.equal(fsm.petitToHiddenDelay, 180);
  assert.equal(fsm.hoverCloseDelay, 0.6);
});

test("hover peeks to compact and never opens the panel by itself", () => {
  const { fsm, timers, seen } = machine();
  fsm.mouseEntered();
  assert.equal(fsm.state, "petit");
  timers.advance(60_000);
  assert.equal(fsm.state, "petit", "resting on it does not expand");
  fsm.mouseLeft();
  timers.advance(14_999);
  assert.equal(fsm.state, "petit");
  timers.advance(1);
  assert.equal(fsm.state, "hidden", "a hover peek folds 15 s after the pointer leaves, like Coucou");
  assert.deepEqual(seen, [
    ["hidden", "petit"],
    ["petit", "hidden"],
  ]);
});

test("re-entering cancels the hide timer", () => {
  const { fsm, timers } = machine();
  fsm.mouseEntered();
  fsm.mouseLeft();
  timers.advance(10_000);
  fsm.mouseEntered();
  timers.advance(500_000);
  assert.equal(fsm.state, "petit");
});

test("a click opens; leaving folds after 15 s, re-entering cancels", () => {
  const { fsm, timers } = machine();
  fsm.mouseEntered();
  fsm.click();
  assert.equal(fsm.state, "home");
  fsm.mouseLeft();
  timers.advance(14_000);
  fsm.mouseEntered();
  timers.advance(60_000);
  assert.equal(fsm.state, "home", "pointer is back on it");
  fsm.mouseLeft();
  timers.advance(15_000);
  assert.equal(fsm.state, "petit");
});

test("open on hover (opt-in) folds 0.6 s after leaving, unless the user clicked inside", () => {
  const { fsm, timers } = machine();
  fsm.openOnHover = true;
  fsm.mouseEntered();
  assert.equal(fsm.state, "home");
  assert.equal(fsm.openedByHover, true);
  fsm.mouseLeft();
  timers.advance(599);
  assert.equal(fsm.state, "home");
  timers.advance(1);
  assert.equal(fsm.state, "petit");

  const b = machine();
  b.fsm.openOnHover = true;
  b.fsm.mouseEntered();
  b.fsm.userInteracted();
  b.fsm.mouseLeft();
  b.timers.advance(600);
  assert.equal(b.fsm.state, "home");
  b.timers.advance(15_000);
  assert.equal(b.fsm.state, "petit");
});

test("an approval holds the island open: no fold, no hide", () => {
  const { fsm, timers } = machine();
  let pending = true;
  fsm.isHeldOpen = () => pending;
  fsm.openedExternally();
  assert.equal(fsm.state, "home");
  fsm.mouseLeft();
  timers.advance(600_000);
  assert.equal(fsm.state, "home");
  pending = false;
  fsm.collapse();
  assert.equal(fsm.state, "petit");
});

test("held open while hidden: entering syncs to home without a transition", () => {
  const { fsm, seen } = machine();
  fsm.isHeldOpen = () => true;
  fsm.mouseEntered();
  assert.equal(fsm.state, "home");
  assert.deepEqual(seen, []);
});

test("reveal shows compact from hidden and schedules the hide; hiddenExternally mirrors silently", () => {
  const { fsm, timers, seen } = machine();
  fsm.reveal();
  assert.equal(fsm.state, "petit");
  fsm.reveal();
  timers.advance(180_000);
  assert.equal(fsm.state, "hidden");
  fsm.reveal();
  fsm.hiddenExternally();
  assert.equal(fsm.state, "hidden");
  assert.equal(seen.length, 3, "hiddenExternally fires no transition");
});

test("changing the auto-close delay while counting restarts it", () => {
  const { fsm, timers } = machine();
  fsm.click();
  fsm.mouseLeft();
  timers.advance(10_000);
  fsm.homeToPetitDelay = 30;
  timers.advance(20_000);
  assert.equal(fsm.state, "home");
  timers.advance(10_000);
  assert.equal(fsm.state, "petit");
});

// ── Geometry (islandSize, IslandConst, IslandPlacement) ─────────────────────

const mac: IslandInfo = {
  hasNotch: true,
  width: 185,
  height: 32,
  compactExtra: 160,
  panel: [720, 560],
  position: "top-center",
  platform: "macos",
};
const pc: IslandInfo = { ...mac, hasNotch: false, width: 184, height: 32, compactExtra: 104, panel: [720, 320], platform: "windows" };

test("Mac notch sizes: hidden = notch, compact = notch + 160, expanded = 640×160", () => {
  assert.deepEqual(islandSize(mac, "hidden", "empty"), { w: 185, h: 32 });
  assert.deepEqual(islandSize(mac, "compact", "empty"), { w: 345, h: 32 });
  assert.deepEqual(islandSize(mac, "expanded", "approval"), { w: 640, h: 160 });
  assert.deepEqual(islandSize(mac, "expanded", "overview"), { w: 640, h: 160 });
  assert.equal(islandFillOpacity(mac, "hidden"), 0, "nothing drawn over a real notch");
  assert.equal(islandFillOpacity(mac, "compact"), 1);
  assert.equal(islandCorner(mac, "compact"), 14);
  assert.equal(islandCorner(mac, "expanded"), 22);
  assert.equal(islandTopRadius(mac, "expanded"), 0, "flat top at the notch");
});

test("PC sizes: hidden retracts into the top edge, compact 288×32", () => {
  assert.deepEqual(islandSize(pc, "hidden", "empty"), { w: 184, h: 0 });
  assert.deepEqual(islandSize(pc, "compact", "empty"), { w: 288, h: 32 });
  assert.deepEqual(islandSize(pc, "expanded", "approval"), { w: 640, h: 160 });
});

test("corners: a 60 pt tile at rest, rounded top, home 640×260", () => {
  const corner: IslandInfo = { ...mac, position: "top-right" };
  assert.deepEqual(islandSize(corner, "compact", "empty"), { w: 60, h: 60 });
  assert.deepEqual(islandSize(corner, "expanded", "overview"), { w: 640, h: 260 });
  assert.equal(islandCorner(corner, "compact"), 18);
  assert.equal(islandTopRadius(corner, "compact"), 18);
  assert.deepEqual(islandRect(corner, 60, 60), { x: 720 - 40 - 60, y: 0, w: 60, h: 60 });
  assert.deepEqual(islandRect(mac, 345, 32), { x: (720 - 345) / 2, y: 0, w: 345, h: 32 });
});

test("mascot and mini grid sit where Coucou draws Mochi and the session grid", () => {
  assert.deepEqual(botPosition("compact", "empty", 345, 32), { cx: 40, cy: 16, diameter: 20, opacity: 1 });
  assert.deepEqual(botPosition("expanded", "approval", 640, 160), { cx: 62, cy: 101, diameter: 56, opacity: 1 });
  assert.equal(botPosition("hidden", "empty", 185, 32).opacity, 0);
  assert.deepEqual(miniGrid(345, 32), { cx: 305, cy: 16, scale: 1 });
  assert.equal(miniGrid(240, 24).scale, 20 / 28);
});

// ── Shape (IslandShape.path) ────────────────────────────────────────────────

test("flat-top notch shape: straight top, rounded bottom corners", () => {
  assert.equal(
    islandPath(345, 32, 14, 0),
    "M 0 0 L 345 0 A 0 0 0 0 1 345 0 L 345 18 A 14 14 0 0 1 331 32 L 14 32 A 14 14 0 0 1 0 18 L 0 0 A 0 0 0 0 1 0 0 Z",
  );
});

test("convex top for a corner tile, clamped to half the size", () => {
  const d = islandPath(60, 60, 18, 18);
  assert.ok(d.startsWith("M 18 0 L 42 0 A 18 18 0 0 1 60 18 "), d);
  assert.ok(islandPath(20, 10, 18, 18).startsWith("M 5 0 "), "top radius clamps to h/2");
});

test("concave ears and empty sizes", () => {
  assert.ok(islandPath(200, 32, 14, -14).startsWith("M 0 0 A 14 14 0 0 1 14 14 L 186 14 A 14 14 0 0 1 200 0 "));
  assert.equal(islandPath(184, 0, 14, 0), "");
});

// ── Animation (SwiftUI spring and close curve) ──────────────────────────────

test("open spring overshoots a little and settles; close curve is monotonic", () => {
  const s = new Spring(0);
  s.target = 100;
  let peak = 0;
  for (let i = 0; i < 120; i++) {
    s.step(1 / 60);
    peak = Math.max(peak, s.value);
  }
  assert.ok(peak > 100 && peak < 110, `peak ${peak}`);
  assert.ok(s.settled);
  let prev = 0;
  for (let x = 0; x <= 1; x += 0.05) {
    const y = closeCurve(x);
    assert.ok(y >= prev - 1e-9);
    prev = y;
  }
});

test("Tracked shrinks along the 340 ms curve and grows on the spring", () => {
  const t = new Tracked(640);
  t.curveTowards(345, 0);
  t.step(1 / 60, CLOSE_MS / 2);
  assert.ok(t.value < 640 && t.value > 345);
  t.step(1 / 60, CLOSE_MS);
  assert.ok(Math.abs(t.value - 345) < 0.01, `${t.value}`);
  assert.equal(t.animating, false);
  t.springTo(640);
  assert.equal(t.animating, true);
  for (let i = 0; i < 200; i++) t.step(1 / 60, 0);
  assert.equal(t.value, 640);
});
