// Next Agent: the gradient orb drawn where Coucou draws its character.
//
// Coucou's MIT licence covers its code, not the Mochi character (its design,
// look, expressions and animations as a character: Coucou's LICENSE-ASSETS.md).
// Next Agent therefore draws this neutral orb wherever Coucou draws Mochi —
// engine.ts (BotEngine.draw), greeting.ts and outfits.ts — in Mochi's place,
// size and motion. Same look as apps/desktop/src/lib/components/IslandMascot.svelte:
// a highlight top-left over a conic gradient of the state's colour, with a
// soft glow. The owner's own artwork replaces it (docs/upstream/Coucou art to
// replace.md).

/** Components 0…1, as BotEngine's colours. */
export type OrbRGB = readonly [number, number, number];

const css = (c: OrbRGB, a = 1) =>
  `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${a})`;

/** The orb's second colour: the same hue, darker. */
function shade(c: OrbRGB, k = 0.62): OrbRGB {
  return [c[0] * k, c[1] * k, c[2] * k];
}

/**
 * Draws an orb of radius `r` centred on the origin of the current transform.
 * `glow` 0…1 is the halo's strength.
 */
export function drawOrb(x: CanvasRenderingContext2D, r: number, color: OrbRGB, glow = 0.35, alpha = 1) {
  if (r <= 0.2 || alpha <= 0.001) return;
  x.save();
  x.globalAlpha *= alpha;
  if (glow > 0.001) {
    const halo = x.createRadialGradient(0, 0, r * 0.6, 0, 0, r * 1.6);
    halo.addColorStop(0, css(color, 0.45 * glow));
    halo.addColorStop(1, css(color, 0));
    x.fillStyle = halo;
    x.beginPath();
    x.arc(0, 0, r * 1.6, 0, Math.PI * 2);
    x.fill();
  }
  const body = new Path2D();
  body.arc(0, 0, r, 0, Math.PI * 2);
  const dark = shade(color);
  const conic = typeof x.createConicGradient === "function" ? x.createConicGradient((210 * Math.PI) / 180, 0, 0) : null;
  if (conic) {
    conic.addColorStop(0, css(color));
    conic.addColorStop(0.5, css(dark));
    conic.addColorStop(1, css(color));
    x.fillStyle = conic;
  } else {
    const lin = x.createLinearGradient(-r, -r, r, r);
    lin.addColorStop(0, css(color));
    lin.addColorStop(1, css(dark));
    x.fillStyle = lin;
  }
  x.fill(body);
  const shine = x.createRadialGradient(-r * 0.36, -r * 0.44, 0, -r * 0.36, -r * 0.44, r * 0.84);
  shine.addColorStop(0, "rgba(255,255,255,0.55)");
  shine.addColorStop(1, "rgba(255,255,255,0)");
  x.fillStyle = shine;
  x.fill(body);
  x.restore();
}

/** `#RRGGBB` → 0…1 components. */
export function orbHex(hex: string): OrbRGB {
  const v = parseInt(hex.replace("#", ""), 16);
  return [((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255];
}
