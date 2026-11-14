// Rules that stop autosave from destroying a board.

export interface SaveGuardState {
  /** The file on disk has at least one live element. */
  diskHasContent: boolean;
  /** The user explicitly agreed to replace a non-empty board with an empty one. */
  allowEmpty: boolean;
}

export type SaveVerdict = "save" | "empty-held";

/**
 * May a scene with `liveCount` live elements be written? An accidental empty
 * scene (for example from a canvas that was torn down) must never replace a
 * board that has content. A destroyed canvas is refused earlier, by dropping
 * the API handle in the page, so `snapshot()` returns null for it.
 */
export function judgeSave(liveCount: number, state: SaveGuardState): SaveVerdict {
  if (liveCount === 0 && state.diskHasContent && !state.allowEmpty) return "empty-held";
  return "save";
}
