// Debounced autosave. `save` runs after `delayMs` of quiet; a save already in
// flight is never overlapped (the newest value is written right after it).

export type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error";

export interface AutosaveOptions<T> {
  save: (value: T) => Promise<void>;
  delayMs?: number;
  onStatus?: (status: SaveStatus, error?: unknown) => void;
}

export interface Autosaver<T> {
  /** Queue a value; resets the debounce timer. */
  schedule(value: T): void;
  /** Save any pending value now and wait for it. */
  flush(): Promise<void>;
  /** Drop the pending value without saving (e.g. after a conflict reload). */
  cancel(): void;
  readonly pending: boolean;
}

export function createAutosaver<T>(options: AutosaveOptions<T>): Autosaver<T> {
  const delay = options.delayMs ?? 800;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pendingValue: { value: T } | null = null;
  let running: Promise<void> | null = null;
  const status = (s: SaveStatus, e?: unknown) => options.onStatus?.(s, e);

  const clear = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  async function run(): Promise<void> {
    while (pendingValue) {
      const { value } = pendingValue;
      pendingValue = null;
      status("saving");
      try {
        await options.save(value);
        status(pendingValue ? "dirty" : "saved");
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
    schedule(value) {
      pendingValue = { value };
      status("dirty");
      clear();
      timer = setTimeout(() => {
        timer = null;
        void start();
      }, delay);
    },
    async flush() {
      clear();
      if (pendingValue) await start();
      else if (running) await running;
    },
    cancel() {
      clear();
      pendingValue = null;
    },
    get pending() {
      return pendingValue !== null || running !== null;
    },
  };
}
