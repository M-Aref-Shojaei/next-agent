// Undo/redo as state snapshots. Typing in one block merges into one step
// (same `key` within `windowMs`); every structural edit is its own step.

export interface History<T> {
  /** Call with the state *before* a change. */
  record(before: T, key?: string): void;
  undo(current: T): T | undefined;
  redo(current: T): T | undefined;
  clear(): void;
  readonly canUndo: boolean;
  readonly canRedo: boolean;
}

export function createHistory<T>(options: { limit?: number; windowMs?: number; now?: () => number } = {}): History<T> {
  const limit = options.limit ?? 200;
  const windowMs = options.windowMs ?? 1000;
  const now = options.now ?? Date.now;
  let past: T[] = [];
  let future: T[] = [];
  let lastKey: string | undefined;
  let lastAt = 0;

  return {
    record(before, key) {
      const t = now();
      const merge = key !== undefined && key === lastKey && t - lastAt <= windowMs && past.length > 0;
      lastKey = key;
      lastAt = t;
      future = [];
      if (merge) return;
      past.push(before);
      if (past.length > limit) past = past.slice(past.length - limit);
    },
    undo(current) {
      const previous = past.pop();
      if (previous === undefined) return undefined;
      future.push(current);
      lastKey = undefined;
      return previous;
    },
    redo(current) {
      const next = future.pop();
      if (next === undefined) return undefined;
      past.push(current);
      lastKey = undefined;
      return next;
    },
    clear() {
      past = [];
      future = [];
      lastKey = undefined;
    },
    get canUndo() {
      return past.length > 0;
    },
    get canRedo() {
      return future.length > 0;
    },
  };
}
