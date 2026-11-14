// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandWindowController.swift (islandSize), NotchBuddy/Sources/CoucouKit/IslandTypes.swift (IslandConst), NotchBuddy/Sources/CoucouKit/IslandScreenGeometry.swift (IslandRestingLayout), NotchBuddy/Sources/App/IslandRootView.swift (islandCorner) and windows/src/core/layout.ts (botPosition, the PC sizes) @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Island geometry in logical px (= macOS points). Pure.

import { EXPANDED_CORNER, ROUNDED_CORNER, TILE_CORNER } from "./shape.ts";

export type IslandMode = "hidden" | "compact" | "expanded";
/** The Coucou views our overlay has: home with sessions, home without, a permission request. */
export type IslandView = "overview" | "empty" | "approval";
export type IslandPosition = "top-center" | "top-left" | "top-right";

/** Resting island and panel, as `overlay_info` / `overlay-geometry` report it. */
export interface IslandInfo {
  hasNotch: boolean;
  /** Resting island: the notch (`nw`, `nh`), or the no-notch bar. */
  width: number;
  height: number;
  /** Compact = resting width + this (Mac: 160; PC: 104). */
  compactExtra: number;
  panel: [number, number];
  position: IslandPosition;
  platform: "macos" | "windows" | "linux";
}

/**
 * Owner's change to Coucou: at rest the island is the notch plus a small wing
 * on each side, with the gradient orb (in Mochi's place) in the left wing, so
 * it is always visible that Next Agent is open.
 */
export const REST_WING = 26;
export const REST_ORB = 12;

/** IslandConst.expandedWidth. */
export const EXPANDED_W = 640;
/** IslandConst.viewLayouts (heights, Mochi x and diameter). */
export const VIEW_LAYOUTS: Record<IslandView, { height: number; botX: number; botDiameter: number }> = {
  overview: { height: 160, botX: 68, botDiameter: 58 },
  empty: { height: 160, botX: 70, botDiameter: 62 },
  approval: { height: 160, botX: 62, botDiameter: 56 },
};
/** IslandPlacement.tile: a resting island in a corner is a square tile. */
export const TILE = 60;
/** IslandPlacement.homeSize: the home view open in a corner. */
export const CORNER_HOME = { width: 640, height: 260 };
/** IslandPlacement.side: room between the panel edge and a corner island. */
export const SIDE = 40;
/** Header: 8 pt top inset + 34 pt header (IslandHeader). */
export const HEADER_BOTTOM = 42;
/** The fixed card height Mochi is centred on. */
export const CARD_H = 84;

export const FALLBACK_INFO: IslandInfo = {
  hasNotch: false,
  width: 184,
  height: 32,
  compactExtra: 104,
  panel: [720, 320],
  position: "top-center",
  platform: "windows",
};

const isCorner = (info: IslandInfo) => info.position !== "top-center";

/** `islandSize(mode:view:)` with the real notch dimensions. */
export function islandSize(info: IslandInfo, mode: IslandMode, view: IslandView): { w: number; h: number } {
  const corner = isCorner(info);
  const nw = info.width;
  const nh = info.height;
  switch (mode) {
    case "hidden":
      if (corner) return { w: TILE, h: TILE };
      // No notch on a PC: the island retracts to zero height into the top edge
      // (windows/src/core/layout.ts); the Mac keeps the notch / resting bar size.
      return info.platform === "macos" ? { w: nw + 2 * REST_WING, h: nh } : { w: nw, h: 0 };
    case "compact":
      return corner ? { w: TILE, h: TILE } : { w: nw + info.compactExtra, h: nh };
    case "expanded":
      if (corner && view !== "approval") return { w: CORNER_HOME.width, h: CORNER_HOME.height };
      return { w: EXPANDED_W, h: VIEW_LAYOUTS[view].height };
  }
}

/** `islandCorner(_:)`: the bottom corner radius. */
export function islandCorner(info: IslandInfo, mode: IslandMode): number {
  if (mode === "expanded") return EXPANDED_CORNER;
  return isCorner(info) ? TILE_CORNER : ROUNDED_CORNER;
}

/** In a corner the island floats: its top corners are rounded too. */
export function islandTopRadius(info: IslandInfo, mode: IslandMode): number {
  return isCorner(info) ? islandCorner(info, mode) : 0;
}

/** Hidden over a real notch: draw nothing (a black copy would slide away with the Space). */
export function islandFillOpacity(info: IslandInfo, mode: IslandMode): number {
  // Owner's change: the resting wings are drawn so the orb sits on black.
  void info;
  void mode;
  return 1;
}

/** The island's rect inside the panel, y down (IslandPlacement.islandRect). */
export function islandRect(info: IslandInfo, w: number, h: number): { x: number; y: number; w: number; h: number } {
  const pw = info.panel[0];
  const x = info.position === "top-left" ? SIDE : info.position === "top-right" ? pw - SIDE - w : (pw - w) / 2;
  return { x, y: 0, w, h };
}

/** Where the mascot sits (`botPosition`): its centre from the island's top-left, and its size. */
export function botPosition(
  mode: IslandMode,
  view: IslandView,
  islandW: number,
  islandH: number,
): { cx: number; cy: number; diameter: number; opacity: number } {
  switch (mode) {
    case "hidden":
      // Owner's change: the resting orb, centred in the left wing.
      return { cx: REST_WING / 2, cy: islandH / 2, diameter: REST_ORB, opacity: 1 };
    case "compact": {
      // IslandRestingLayout.botDiameter / botCenterY.
      return { cx: 40, cy: islandH / 2, diameter: Math.min(20, Math.max(0, islandH - 6)), opacity: 1 };
    }
    case "expanded": {
      const layout = VIEW_LAYOUTS[view];
      const cy = HEADER_BOTTOM + (islandH - HEADER_BOTTOM - CARD_H) / 2 + CARD_H / 2;
      void islandW;
      return { cx: layout.botX, cy, diameter: layout.botDiameter, opacity: 1 };
    }
  }
}

/** The compact 2×2 grid of sessions to the right of the notch (CompactMiniGrid). */
export function miniGrid(islandW: number, islandH: number): { cx: number; cy: number; scale: number } {
  return { cx: islandW - 40, cy: islandH / 2, scale: Math.min(1, Math.max(0, islandH - 4) / 28) };
}
