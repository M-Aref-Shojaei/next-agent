// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandRootView.swift (IslandShape) @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// The island outline as an SVG path, the same geometry as Coucou's `IslandShape`:
//   topRadius > 0  convex rounded top corners (a floating island in a corner)
//   topRadius < 0  concave ear cut-outs, |topRadius| = ear radius
//   topRadius = 0  sharp top corners (at the notch)
// `cornerRadius` is the radius of the two bottom corners.

/** IslandConst: hidden / peek / compact. */
export const ROUNDED_CORNER = 14;
/** IslandConst: expanded. */
export const EXPANDED_CORNER = 22;
/** IslandConst.earRadius. */
export const EAR_RADIUS = 14;
/** IslandPlacement.tileCorner (a resting island in a corner). */
export const TILE_CORNER = 18;

const n = (v: number) => (Math.round(v * 100) / 100).toString();

/** SVG `d` for an island of `width × height`, origin top-left. */
export function islandPath(width: number, height: number, cornerRadius: number, topRadius: number): string {
  if (!(width > 0) || !(height > 0)) return "";
  const cr = Math.max(0, cornerRadius);
  // Bottom edge and corners, shared by both variants (from the right edge to the left edge).
  const lower = (leftTop: number) =>
    `L ${n(width)} ${n(height - cr)} ` +
    `A ${n(cr)} ${n(cr)} 0 0 1 ${n(width - cr)} ${n(height)} ` +
    `L ${n(cr)} ${n(height)} ` +
    `A ${n(cr)} ${n(cr)} 0 0 1 0 ${n(height - cr)} ` +
    `L 0 ${n(leftTop)}`;

  if (topRadius >= 0) {
    // Convex rounded top corners (expanded / corner).
    const tr = Math.min(topRadius, Math.min(width / 2, height / 2));
    return (
      `M ${n(tr)} 0 L ${n(width - tr)} 0 ` +
      `A ${n(tr)} ${n(tr)} 0 0 1 ${n(width)} ${n(tr)} ` +
      lower(tr) +
      ` A ${n(tr)} ${n(tr)} 0 0 1 ${n(tr)} 0 Z`
    );
  }
  // Concave ear cutouts (compact / notch).
  const er = -topRadius;
  return (
    `M 0 0 A ${n(er)} ${n(er)} 0 0 1 ${n(er)} ${n(er)} ` +
    `L ${n(width - er)} ${n(er)} ` +
    `A ${n(er)} ${n(er)} 0 0 1 ${n(width)} 0 ` +
    lower(0) +
    " Z"
  );
}
