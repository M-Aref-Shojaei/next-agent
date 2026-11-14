import type { Skeleton } from "./skeleton.ts";

export interface StickyPreset {
  id: string;
  label: string;
  color: string;
}

/** Sticky-note colours (Excalidraw's light palette, readable with dark text). */
export const STICKY_PRESETS: readonly StickyPreset[] = [
  { id: "yellow", label: "Yellow", color: "#ffec99" },
  { id: "orange", label: "Orange", color: "#ffd8a8" },
  { id: "pink", label: "Pink", color: "#ffc9c9" },
  { id: "green", label: "Green", color: "#b2f2bb" },
  { id: "blue", label: "Blue", color: "#a5d8ff" },
  { id: "purple", label: "Purple", color: "#d0bfff" },
];

export const STICKY_SIZE = 180;

/** A sticky note: a borderless, solid-filled square with centred text. */
export function sticky(color: string, text = "Idea", x = 0, y = 0, id?: string): Skeleton {
  return {
    ...(id === undefined ? {} : { id }),
    type: "rectangle",
    x,
    y,
    width: STICKY_SIZE,
    height: STICKY_SIZE,
    backgroundColor: color,
    strokeColor: "transparent",
    fillStyle: "solid",
    roughness: 0,
    roundness: { type: 3 },
    label: { text, fontSize: 20, textAlign: "center", verticalAlign: "middle", strokeColor: "#1e1e1e" },
  };
}
