// Plain-data description of board content (Excalidraw "element skeletons").
// Library items and templates are authored in this form by us; they are turned
// into real elements at runtime with Excalidraw's convertToExcalidrawElements.
// Keeping them as data lets us validate and test them without a browser.

export type SkeletonType = "rectangle" | "ellipse" | "diamond" | "text" | "line" | "arrow" | "frame";

export interface Label {
  text: string;
  fontSize?: number;
  textAlign?: "left" | "center" | "right";
  verticalAlign?: "top" | "middle";
  strokeColor?: string;
}

export interface Skeleton {
  type: SkeletonType;
  id?: string;
  x: number;
  y: number;
  width?: number;
  height?: number;
  text?: string;
  fontSize?: number;
  strokeColor?: string;
  backgroundColor?: string;
  fillStyle?: "solid" | "hachure" | "cross-hatch";
  strokeStyle?: "solid" | "dashed" | "dotted";
  strokeWidth?: number;
  roughness?: number;
  roundness?: { type: number } | null;
  label?: Label;
  /** arrow/line only: relative points; arrows may bind to element ids */
  points?: [number, number][];
  start?: { id: string };
  end?: { id: string };
  endArrowhead?: string | null;
  /** frame only */
  name?: string;
  children?: string[];
}

const TYPES: ReadonlySet<string> = new Set(["rectangle", "ellipse", "diamond", "text", "line", "arrow", "frame"]);

/** Problems found in a skeleton list; empty means valid. */
export function validateSkeletons(items: readonly Skeleton[]): string[] {
  const problems: string[] = [];
  const ids = new Set<string>();
  for (const item of items) {
    if (item.id !== undefined) {
      if (ids.has(item.id)) problems.push(`duplicate id ${item.id}`);
      ids.add(item.id);
    }
  }
  items.forEach((item, i) => {
    const at = `#${i} (${item.type}${item.id ? ` ${item.id}` : ""})`;
    if (!TYPES.has(item.type)) problems.push(`${at}: unknown type`);
    if (!Number.isFinite(item.x) || !Number.isFinite(item.y)) problems.push(`${at}: x/y must be finite numbers`);
    for (const key of ["width", "height"] as const) {
      const v = item[key];
      if (v !== undefined && !(Number.isFinite(v) && v > 0)) problems.push(`${at}: ${key} must be positive`);
    }
    if (item.type === "text" && !item.text) problems.push(`${at}: text element needs text`);
    if (item.type === "rectangle" || item.type === "ellipse" || item.type === "diamond") {
      if (item.width === undefined || item.height === undefined) problems.push(`${at}: shape needs width and height`);
    }
    if (item.label && !item.label.text) problems.push(`${at}: empty label`);
    for (const end of [item.start, item.end]) {
      if (end && !ids.has(end.id)) problems.push(`${at}: arrow binds to missing id ${end.id}`);
    }
    if (item.type === "arrow" || item.type === "line") {
      const hasEnds = item.start !== undefined && item.end !== undefined;
      if (!hasEnds && !(item.points && item.points.length >= 2) && item.width === undefined && item.height === undefined) {
        problems.push(`${at}: needs points, width/height, or start+end`);
      }
    }
    if (item.type === "frame") {
      if (!item.children?.length && (item.width === undefined || item.height === undefined)) {
        problems.push(`${at}: frame needs children or a size`);
      }
      for (const child of item.children ?? []) {
        if (!ids.has(child)) problems.push(`${at}: frame child ${child} missing`);
      }
    }
  });
  return problems;
}

/** Move a skeleton group so the centre of its bounding box lands on (cx, cy). */
export function placeAt(items: readonly Skeleton[], cx: number, cy: number): Skeleton[] {
  const boxed = items.filter((i) => i.type !== "arrow" || i.width !== undefined);
  if (boxed.length === 0) return items.map((i) => ({ ...i }));
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const i of boxed) {
    minX = Math.min(minX, i.x);
    minY = Math.min(minY, i.y);
    maxX = Math.max(maxX, i.x + (i.width ?? 0));
    maxY = Math.max(maxY, i.y + (i.height ?? 0));
  }
  const dx = cx - (minX + maxX) / 2;
  const dy = cy - (minY + maxY) / 2;
  return items.map((i) => ({ ...i, x: i.x + dx, y: i.y + dy }));
}

/** Give every id in the group a fresh prefix so repeated inserts never collide. */
export function reId(items: readonly Skeleton[], prefix: string): Skeleton[] {
  const map = (id: string) => `${prefix}${id}`;
  return items.map((i) => ({
    ...i,
    id: i.id === undefined ? undefined : map(i.id),
    start: i.start ? { id: map(i.start.id) } : undefined,
    end: i.end ? { id: map(i.end.id) } : undefined,
    children: i.children?.map(map),
  }));
}
