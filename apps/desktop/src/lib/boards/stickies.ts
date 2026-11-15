import { INK, STICKY_COLOURS } from "./palette-data.ts";
import type { Skeleton } from "./skeleton.ts";

export interface StickyPreset {
  id: string;
  label: string;
  color: string;
}

/** Sticky-note colours (Excalidraw's light palette, readable with dark text). */
export const STICKY_PRESETS: readonly StickyPreset[] = (Object.keys(STICKY_COLOURS) as (keyof typeof STICKY_COLOURS)[]).map((id) => ({
  id,
  label: id[0]!.toUpperCase() + id.slice(1),
  color: STICKY_COLOURS[id],
}));

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
    label: { text, fontSize: 20, textAlign: "center", verticalAlign: "middle", strokeColor: INK },
  };
}
