// Colours that are Excalidraw scene DATA, not UI chrome: they are written into
// the user's .excalidraw files (sticky notes, wireframe library, templates) and
// must stay the same in light and dark, so they cannot be design tokens.
// Keys are deliberately not CSS property names: the design-token checker reads
// style declarations, and these are scene values.

/** Ink and neutrals of the wireframe library (Excalidraw's default palette). */
export const INK = "#1e1e1e";
export const MUTED_INK = "#868e96";
export const PAPER = "#ffffff";
export const PAPER_TINT = "#f1f3f5";
/** Interactive elements in wireframes (buttons, toggles). */
export const WIRE_ACCENT = "#4c6ef5";

/** Sticky notes: Excalidraw's light palette, readable with dark text. */
export const STICKY_COLOURS = {
  yellow: "#ffec99",
  orange: "#ffd8a8",
  pink: "#ffc9c9",
  green: "#b2f2bb",
  blue: "#a5d8ff",
  purple: "#d0bfff",
} as const;

/** Shape tints of the user-flow template. */
export const FLOW_TINTS = {
  step: "#e7f5ff",
  start: "#d3f9d8",
  decision: "#fff3bf",
  end: "#ffe3e3",
} as const;
