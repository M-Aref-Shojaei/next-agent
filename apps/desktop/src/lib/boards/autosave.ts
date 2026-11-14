// Debounced autosave. `save` runs after `delayMs` of quiet; a save already in
// flight is never overlapped (the newest value is written right after it).
//
// `schedule` takes a getter, not a value, so a large snapshot is built once per
// save rather than once per change. The getter always runs synchronously: when
// the timer fires, or at the start of `flush()` (which freezes it before its
// first await). A caller that must tear its source down can therefore
// `void flush()` first and be sure the value was captured while it still existed.

export type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";

export interface AutosaveOptions<T> {
  save: (value: T) => Promise<void>;
  delayMs?: number;
  onStatus?: (status: SaveStatus, error?: unknown) => void;
}

export interface Autosaver<T> {
  /** Queue a snapshot getter; resets the debounce timer. */
  schedule(get: () => T): void;
  /** Save any pending value now and wait for it. */
  flush(): Promise<void>;
  /** Drop the pending value without saving (e.g. after a conflict reload). */
  cancel(): void;
  readonly pending: boolean;
}

export function createAutosaver<T>(options: AutosaveOptions<T>): Autosaver<T> {
  const delay = options.delayMs ?? 800;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: { get: () => T } | null = null;
  let running: Promise<void> | null = null;
  const status = (s: SaveStatus, e?: unknown) => options.onStatus?.(s, e);

  const clear = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  async function run(): Promise<void> {
    while (pending) {
      const { get } = pending;
      pending = null;
      status("saving");
      try {
        await options.save(get());
        status(pending ? "dirty" : "saved");
      } catch (error) {
        status("error", error);
        return; // keep nothing queued; the next change schedules a new save
      }
    }
  }

  function start(): Promise<void> {
    if (!running) {
      running = run().finally(() => {
        running = null;
      });
    }
    return running;
  }

  return {
    schedule(get) {
      pending = { get };
      status("dirty");
      clear();
      timer = setTimeout(() => {
        timer = null;
        void start();
      }, delay);
    },
    async flush() {
      clear();
      if (pending) {
        try {
          const value = pending.get(); // capture now, before anything can tear the source down
          pending = { get: () => value };
        } catch (error) {
          pending = null;
          status("error", error);
          return;
        }
        await start();
      } else if (running) await running;
    },
    cancel() {
      clear();
      pending = null;
    },
    get pending() {
      return pending !== null || running !== null;
    },
  };
}
