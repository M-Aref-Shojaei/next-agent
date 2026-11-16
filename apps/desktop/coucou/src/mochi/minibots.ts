// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src/mochi/minibots.ts @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
// Mini Mochis (pills + compact grid) — port of MiniBotCanvasView.
// Each canvas owns a BotEngine; the island's frame loop ticks every live one.

import { BotEngine, hexToRGB, type RGB } from "./engine";
import type { AgentTask } from "../core/state";
// Next Agent: our own logo mark (src/assets/brand/), not Coucou art.
import MARK_MONO_URL from "../assets/brand/mark-mono.png?url";
import MARK_LINES_URL from "../assets/brand/mark-lines.png?url";

interface MiniBot {
  canvas: HTMLCanvasElement;
  engine: BotEngine;
  cssSize: number;
  taskId: string;
  /** Asked every frame: whether this one dances (the music pill's, MiniBotCanvasView). */
  dancing?: () => boolean;
}

// Next Agent: the pills wear the Next Agent logo mark instead of the orb. `mark-mono` (the hexagon with
// the brain lines cut out) is tinted with the pill's identity colour, `mark-lines` (white) goes on top.
// The images load once; the tinted hexagon is cached per colour.
let markMono: HTMLImageElement | null = null;
let markLines: HTMLImageElement | null = null;
const tintCache = new Map<string, HTMLCanvasElement>();

function loadMark() {
  if (markMono || typeof Image === "undefined") return;
  markMono = new Image();
  markMono.src = MARK_MONO_URL;
  markLines = new Image();
  markLines.src = MARK_LINES_URL;
}

const ready = (i: HTMLImageElement | null): i is HTMLImageElement => !!i && i.complete && i.naturalWidth > 0;

function tintedMark(color: RGB): HTMLCanvasElement | null {
  if (!ready(markMono)) return null;
  const key = color.map((c) => Math.round(c * 255)).join(",");
  let c = tintCache.get(key);
  if (!c) {
    c = document.createElement("canvas");
    c.width = markMono.naturalWidth;
    c.height = markMono.naturalHeight;
    const g = c.getContext("2d")!;
    g.drawImage(markMono, 0, 0);
    g.globalCompositeOperation = "source-in"; // keeps the colour only where the hexagon is
    g.fillStyle = `rgb(${key})`;
    g.fillRect(0, 0, c.width, c.height);
    tintCache.set(key, c);
  }
  return c;
}

/** Draws the mark centred on the origin, `r` being the orb radius it replaces. */
function drawMark(x: CanvasRenderingContext2D, r: number, color: RGB, alpha: number) {
  const hex = tintedMark(color);
  if (!hex || !ready(markLines) || alpha <= 0.001) return;
  const d = r * 2.1;
  x.save();
  x.globalAlpha *= alpha;
  x.imageSmoothingQuality = "high";
  x.drawImage(hex, -d / 2, -d / 2, d, d);
  x.drawImage(markLines, -d / 2, -d / 2, d, d);
  x.restore();
}

const live = new Map<HTMLCanvasElement, MiniBot>();

/**
 * Creates a mini Mochi whose **body** is `bodySize` CSS pixels across.
 *
 * The engine draws the body at 60 % of its canvas, so the canvas is
 * `bodySize / 0.6` and is centred in a `bodySize` slot, overflowing it — the
 * same thing SwiftUI does with a `.frame(width: 22/0.6)` inside a
 * `.frame(width: 22)`. Sizing the canvas itself to `bodySize` would shrink the
 * whole drawing to 60 %, which is what used to happen.
 */
export function createMiniBot(task: AgentTask, bodySize: number, dancing?: () => boolean): HTMLElement {
  const slot = document.createElement("span");
  slot.className = "mini";
  slot.style.width = `${bodySize}px`;
  slot.style.height = `${bodySize}px`;

  const canvas = document.createElement("canvas");
  const engineSize = bodySize / 0.6;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  canvas.width = Math.round(engineSize * dpr);
  canvas.height = Math.round(engineSize * dpr);
  canvas.style.width = `${engineSize}px`;
  canvas.style.height = `${engineSize}px`;
  slot.append(canvas);

  const engine = new BotEngine();
  engine.isMini = true;
  loadMark();
  engine.bodyDrawer = drawMark;
  engine.bodyColor = hexToRGB(task.color);
  engine.setState(task.state, true);
  if (task.emote) engine.setPermanentEmote(task.emote);
  if (task.miniEye) {
    engine.permanentEye = task.miniEye;
    engine.eyeOverride = task.miniEye;
    engine.eyeOverrideUntil = Number.POSITIVE_INFINITY;
  }

  live.set(canvas, { canvas, engine, cssSize: engineSize, taskId: task.id, dancing });
  return slot;
}

export function releaseMiniBot(canvas: HTMLCanvasElement) {
  live.delete(canvas);
}

/** Drops every canvas no longer in the document (views are rebuilt wholesale). */
export function pruneMiniBots() {
  for (const [canvas] of live) {
    if (!canvas.isConnected) live.delete(canvas);
  }
}

export function syncMiniBotStates(tasks: AgentTask[]) {
  for (const mb of live.values()) {
    const task = tasks.find((t) => t.id === mb.taskId);
    if (!task) continue;
    mb.engine.setState(task.state);
    mb.engine.bodyColor = hexToRGB(task.color);
  }
}

export function tickMiniBots(dt: number) {
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  for (const mb of live.values()) {
    const ctx = mb.canvas.getContext("2d");
    if (!ctx) continue;
    mb.engine.setDancing(mb.dancing?.() ?? false);
    mb.engine.update(dt);
    // Sized at creation: the island may have moved to a monitor of another scale.
    const px = Math.round(mb.cssSize * dpr);
    if (mb.canvas.width !== px) {
      mb.canvas.width = px;
      mb.canvas.height = px;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, mb.cssSize, mb.cssSize);
    ctx.save();
    mb.engine.applyDance(ctx, mb.cssSize, mb.cssSize);
    mb.engine.draw(ctx, mb.cssSize, mb.cssSize);
    ctx.restore();
  }
}

export const miniBotCount = () => live.size;
