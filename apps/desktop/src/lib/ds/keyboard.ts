// Roving-focus keyboard model shared by <SegmentedControl> and <Tabs>
// (WAI-ARIA radiogroup / tablist patterns). Pure, so it is tested without a DOM.

export type Orientation = "horizontal" | "vertical";

/**
 * The index the focus moves to for `key`, skipping disabled items and
 * wrapping around; `null` when the key is not a navigation key.
 */
export function rovingIndex(
  key: string,
  current: number,
  disabled: readonly boolean[],
  orientation: Orientation = "horizontal",
): number | null {
  const count = disabled.length;
  if (count === 0 || disabled.every(Boolean)) return null;
  const prev = orientation === "horizontal" ? "ArrowLeft" : "ArrowUp";
  const next = orientation === "horizontal" ? "ArrowRight" : "ArrowDown";
  const step = (from: number, dir: 1 | -1) => {
    let i = from;
    do i = (i + dir + count) % count;
    while (disabled[i]);
    return i;
  };
  switch (key) {
    case next: return step(current, 1);
    case prev: return step(current, -1);
    case "Home": return step(-1, 1);
    case "End": return step(count, -1);
    default: return null;
  }
}
