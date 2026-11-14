import { getContext, setContext } from "svelte";

/** What the notes components need from the page that hosts them. */
export interface NotesCtx {
  open(name: string): void;
  /** Known page names (for `[[` completion). */
  pageNames(): string[];
  /** `yyyyMMdd` of today. */
  today(): number;
  /** Bumped whenever the index changed (own saves included). */
  version(): number;
}

const KEY = Symbol("notes");
export const provideNotes = (ctx: NotesCtx): void => void setContext(KEY, ctx);
export const useNotes = (): NotesCtx => getContext<NotesCtx>(KEY);
