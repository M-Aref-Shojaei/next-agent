// Notes session: opens the graph in the vault, keeps the page list and a change counter
// that components watch to refresh when files change (our own saves included).
import { tick } from "svelte";
import { api } from "../api.ts";
import { log } from "../log.ts";
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
  private session = 0;
  private opening = Promise.resolve();
  private vault: string | null = null;
  private editorSaves = new Set<Promise<void>>();

  finishEditor(save: Promise<void>): void {
    this.editorSaves.add(save);
    const remove = () => this.editorSaves.delete(save);
    void save.then(remove, remove);
  }

  init(projectVault: string | null = null): Promise<void> {
    this.today = dayNumber(new Date());
    return this.openSession(async () => projectVault ?? await api.vaultGet(), false);
  }

  /** Use `path` as the notes vault (the user said so), laying out a graph if it has none. */
  choose(path: string): Promise<void> {
    return this.openSession(() => api.vaultSet(path.trim()), true);
  }

  async setup(): Promise<void> {
    if (this.vault) {
      const vault = this.vault;
      await this.openSession(async () => vault, true);
    }
  }

  private openSession(resolveVault: () => Promise<string | null>, create: boolean): Promise<void> {
    this.destroy();
    const session = this.session;
    this.phase = "loading";
    this.root = "";
    this.pages = [];
    this.opening = this.opening.then(async () => {
      if (session !== this.session) return;
      try {
        // Loading unmounts the old editors. Drain their queued writes while the
        // backend still has their vault open.
        await tick();
        await Promise.all(this.editorSaves);
        if (session !== this.session) return;
        const vault = await resolveVault();
        if (session !== this.session) return;
        this.vault = vault;
        if (vault) await this.open(vault, create, session);
        else this.phase = "choose";
      } catch (e) {
        if (session === this.session) this.fail(e);
      }
    });
    return this.opening;
  }

  private async open(path: string, create: boolean, session: number): Promise<void> {
    try {
      const info = await api.graphOpen(path, create);
      if (session !== this.session) return;
      this.root = info.root;
    } catch (e) {
      const text = String(e instanceof Error ? e.message : e);
      if (session !== this.session) return;
      if (text.includes("is not a Logseq graph")) {
        this.message = text;
        this.phase = "setup";
        return;
      }
      throw e;
    }
    await this.refreshPages();
    if (session !== this.session) return;
    const next = await api.onGraphChanged((files) => {
      if (session !== this.session) return;
      this.changed = files;
      this.version++;
      if (this.timer) clearTimeout(this.timer);
      this.timer = setTimeout(() => void this.refreshPages(), 250);
    });
    if (session !== this.session) {
      next();
      return;
    }
    this.unlisten = next;
    this.phase = "ready";
  }

  async refreshPages(): Promise<void> {
    const session = this.session;
    try {
      const pages = await api.graphPages();
      if (session === this.session) this.pages = pages;
    } catch {
      /* the page list is a convenience; the views report their own errors */
    }
  }

  destroy(): void {
    this.session++;
    this.unlisten?.();
    this.unlisten = null;
    if (this.timer) clearTimeout(this.timer);
  }

  private fail(e: unknown): void {
    log.warn("ui_error", { kind: "notes" });
    this.message = String(e instanceof Error ? e.message : e);
    this.phase = "error";
  }
}

export const notes = new NotesStore();
