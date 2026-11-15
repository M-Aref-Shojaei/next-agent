// Notes session: opens the graph in the vault, keeps the page list and a change counter
// that components watch to refresh when files change (our own saves included).
import { api } from "../api.ts";
import { dayNumber, type PageListItem } from "./types.ts";

export type Phase = "loading" | "choose" | "setup" | "ready" | "error";

class NotesStore {
  phase = $state<Phase>("loading");
  message = $state("");
  root = $state("");
  pages = $state.raw<PageListItem[]>([]);
  version = $state(0);
  /** Files of the last change event. */
  changed = $state.raw<string[]>([]);
  today = $state(dayNumber(new Date()));
  private unlisten: (() => void) | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  async init(): Promise<void> {
    this.today = dayNumber(new Date());
    this.phase = "loading";
    try {
      const vault = await api.vaultGet();
      if (!vault) {
        this.phase = "choose";
        return;
      }
      await this.open(vault, false);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Use `path` as the notes vault (the user said so), laying out a graph if it has none. */
  async choose(path: string): Promise<void> {
    try {
      const saved = await api.vaultSet(path.trim());
      await this.open(saved, true);
    } catch (e) {
      this.fail(e);
    }
  }

  async setup(): Promise<void> {
    try {
      const vault = await api.vaultGet();
      if (vault) await this.open(vault, true);
    } catch (e) {
      this.fail(e);
    }
  }

  private async open(path: string, create: boolean): Promise<void> {
    try {
      const info = await api.graphOpen(path, create);
      this.root = info.root;
    } catch (e) {
      const text = String(e instanceof Error ? e.message : e);
      if (text.includes("is not a Logseq graph")) {
        this.message = text;
        this.phase = "setup";
        return;
      }
      throw e;
    }
    await this.refreshPages();
    const next = await api.onGraphChanged((files) => {
      this.changed = files;
      this.version++;
      if (this.timer) clearTimeout(this.timer);
      this.timer = setTimeout(() => void this.refreshPages(), 250);
    });
    this.unlisten?.();
    this.unlisten = next;
    this.phase = "ready";
  }

  async refreshPages(): Promise<void> {
    try {
      this.pages = await api.graphPages();
    } catch {
      /* the page list is a convenience; the views report their own errors */
    }
  }

  destroy(): void {
    this.unlisten?.();
    this.unlisten = null;
    if (this.timer) clearTimeout(this.timer);
  }

  private fail(e: unknown): void {
    this.message = String(e instanceof Error ? e.message : e);
    this.phase = "error";
  }
}

export const notes = new NotesStore();
