// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src/mochi/outfits.ts (drawWardrobeIcon, roundRect) @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Next Agent: Coucou's outfits.ts draws Mochi's costumes (the hats, ears and
// the pumpkin), which are part of the Mochi character, outside Coucou's MIT
// licence (its LICENSE-ASSETS.md); they are not copied. What the rest of the
// code calls from here stays: the wardrobe button's picture, now the orb
// (orb.ts) instead of a little dressed Mochi, with upstream's crossed-out
// circle for "none" and its AUTO tag for "auto". Which outfit is picked, the
// seasons and the wardrobe view still work as upstream (wardrobe.ts,
// views/wardrobe.ts); the owner's artwork can draw the outfits again
// (docs/upstream/Coucou art to replace.md).

import type { Outfit, OutfitSelection } from "./wardrobe";
import { SCRIPT_FONTS } from "../core/fonts";
import { drawOrb, orbHex } from "./orb";

type Ctx = CanvasRenderingContext2D;

function roundRect(ctx: Ctx, x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

/** Next Agent: the orb in a `size`×`size` icon, in place of a dressed Mochi. */
function iconOrb(ctx: Ctx, size: number, _outfit: Outfit) {
  const R = 10;
  ctx.save();
  ctx.translate(size / 2, size / 2 + R * 0.62 - R * 0.4);
  drawOrb(ctx, R * 0.9, orbHex("#9398A1"), 0.2);
  ctx.restore();
}

/**
 * One wardrobe button's picture — drawOutfitIcon on macOS. "auto" shows the
 * season's outfit with an AUTO tag, "none" a crossed-out circle.
 */
export function drawWardrobeIcon(ctx: Ctx, size: number, selection: OutfitSelection, seasonal: Outfit, autoLabel: string) {
  ctx.clearRect(0, 0, size, size);
  if (selection === "none") {
    const r = 6.5;
    ctx.save();
    ctx.translate(size / 2, size / 2);
    ctx.strokeStyle = "#454850";
    ctx.lineWidth = 1.4;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.arc(0, 0, r * 0.82, 0, Math.PI * 2);
    ctx.moveTo(-r * 0.56, r * 0.56);
    ctx.lineTo(r * 0.56, -r * 0.56);
    ctx.stroke();
    ctx.restore();
    return;
  }
  iconOrb(ctx, size, selection === "auto" ? seasonal : selection);
  if (selection !== "auto") return;
  const R = 10;
  const by = size / 2 + R * 0.62 + R * 0.88 * 0.72;
  const bw = 14;
  const bh = 6.5;
  ctx.save();
  ctx.translate(size / 2, by);
  roundRect(ctx, -bw / 2, -bh / 2, bw, bh, bh / 2);
  ctx.fillStyle = "rgba(0,0,0,0.6)";
  ctx.fill();
  ctx.fillStyle = "#FFFFFF";
  // A longer translation of "AUTO" gets a smaller font rather than overflowing the badge.
  let fontPx = 4.2;
  ctx.font = `600 ${fontPx}px system-ui, "Segoe UI", ${SCRIPT_FONTS}, sans-serif`;
  while (ctx.measureText(autoLabel).width > bw - 2 && fontPx > 2.6) {
    fontPx -= 0.2;
    ctx.font = `600 ${fontPx}px system-ui, "Segoe UI", ${SCRIPT_FONTS}, sans-serif`;
  }
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(autoLabel, 0, 0.2);
  ctx.restore();
}
