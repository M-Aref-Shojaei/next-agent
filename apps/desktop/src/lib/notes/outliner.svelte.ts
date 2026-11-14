// The editing state of one page: the block tree, which block is being edited, undo/redo and
// autosave. All edits go through the pure operations in outline.ts.
import { createAutosaver, type SaveStatus } from "../boards/autosave.ts";
import { cycleMarker, nextMarker, setMarker, markerOf } from "./content.ts";
import { createHistory } from "./history.ts";
import type { EditorAction } from "./keymap.ts";
import * as ops from "./outline.ts";
import type { BlockNode } from "./types.ts";

interface Snapshot {
  tree: ops.Tree;
  focus: string | null;
}
export interface Focus {
  id: string;
  caret: number | "end";
  /** Changes on every request so the same block can be focused again. */
  nonce: number;
}

export interface OutlinerOptions {
  save: (tree: BlockNode[]) => Promise<void>;
  /** Has the page a file already? Pages that do not are only written once they have text. */
  exists: () => boolean;
  delayMs?: number;
}

export class Outliner {
  tree = $state.raw<ops.Tree>([ops.emptyBlock()]);
  editing = $state.raw<Focus | null>(null);
  status = $state<SaveStatus>("idle");
  error = $state("");
  private history = createHistory<Snapshot>();
  private nonce = 0;
  private saver;

  constructor(private options: OutlinerOptions) {
    this.saver = createAutosaver<BlockNode[] | null>({
      delayMs: options.delayMs ?? 600,
      save: async (nodes) => {
        if (nodes) await options.save(nodes);
      },
      onStatus: (status, error) => {
        this.status = status;
        this.error = status === "error" ? String(error instanceof Error ? error.message : error) : "";
      },
    });
  }

  get pending(): boolean {
    return this.saver.pending;
  }

  load(blocks: readonly BlockNode[]): void {
    this.saver.cancel();
    this.history.clear();
    this.editing = null;
    this.tree = blocks.length ? ops.fromNodes(blocks) : [ops.emptyBlock()];
  }

  /** A file changed on disk: adopt it unless the user is in the middle of something. */
  reloadIfIdle(blocks: readonly BlockNode[]): void {
    if (this.saver.pending || this.editing) return;
    const same = JSON.stringify(ops.toNodes(this.tree)) === JSON.stringify(blocks.map(strip));
    if (!same && !(blocks.length === 0 && !ops.hasContent(this.tree))) this.load(blocks);
  }

  flush(): Promise<void> {
    return this.saver.flush();
  }

  // ------------------------------------------------------------------ edits --

  private snapshot(): Snapshot {
    return { tree: this.tree, focus: this.editing?.id ?? null };
  }

  private commit(next: ops.Tree, focus?: { id: string; caret: number | "end" }, key?: string): void {
    this.history.record(this.snapshot(), key);
    this.tree = next;
    if (focus) this.focus(focus.id, focus.caret);
    this.queueSave();
  }

  private queueSave(): void {
    this.saver.schedule(() => {
      if (!this.options.exists() && !ops.hasContent(this.tree)) return null;
      return ops.toNodes(this.tree);
    });
  }

  focus(id: string, caret: number | "end" = "end"): void {
    this.editing = { id, caret, nonce: ++this.nonce };
  }

  /** The editor of `id` lost focus. `nonce` is the request it was opened by: a block that is
   * being re-created (moved under another parent) blurs after the new request was made. */
  blur(id: string, nonce: number): void {
    if (this.editing?.id === id && this.editing.nonce === nonce) this.editing = null;
  }

  /** Text typed in the block's editor. Typing in one block merges into one undo step. */
  input(id: string, text: string): void {
    const next = ops.setText(this.tree, id, text);
    if (next) this.commit(next, undefined, `type:${id}`);
  }

  toggleCollapsed(id: string, collapsed?: boolean): void {
    const next = ops.toggleCollapsed(this.tree, id, collapsed);
    if (next) this.commit(next);
  }

  /** Click on a task checkbox in the rendered block. */
  advanceTask(id: string): void {
    const loc = ops.locate(this.tree, id);
    const marker = loc && markerOf(loc.block.content);
    if (!loc || !marker) return;
    const next = ops.setContent(this.tree, id, setMarker(loc.block.content, nextMarker(marker)));
    if (next) this.commit(next);
  }

  /** Run an editor action for the block being edited. `caret` is its caret offset in the visible text. */
  act(action: EditorAction, id: string, caret: number): void {
    switch (action) {
      case "split": {
        const out = ops.split(this.tree, id, caret);
        if (out) this.commit(out.tree, { id: out.focus, caret: 0 });
        break;
      }
      case "indent":
      case "outdent":
      case "moveUp":
      case "moveDown": {
        const fn = { indent: ops.indent, outdent: ops.outdent, moveUp: ops.moveUp, moveDown: ops.moveDown }[action];
        const next = fn(this.tree, id);
        if (next) this.commit(next, { id, caret });
        break;
      }
      case "mergePrevious": {
        const out = ops.mergeWithPrevious(this.tree, id);
        if (out) this.commit(out.tree, { id: out.focus, caret: out.caret });
        break;
      }
      case "focusPrevious": {
        const prev = ops.previousVisible(this.tree, id);
        if (prev) this.focus(prev, "end");
        break;
      }
      case "focusNext": {
        const next = ops.nextVisible(this.tree, id);
        if (next) this.focus(next, 0);
        break;
      }
      case "cycleTask": {
        const loc = ops.locate(this.tree, id);
        const next = loc && ops.setContent(this.tree, id, cycleMarker(loc.block.content));
        if (next) this.commit(next, { id, caret: "end" });
        break;
      }
      case "collapse":
      case "expand": {
        const loc = ops.locate(this.tree, id);
        if (loc && loc.block.children.length) this.toggleCollapsed(id, action === "collapse");
        break;
      }
      case "undo":
        this.undo();
        break;
      case "redo":
        this.redo();
        break;
      case "exit":
        this.editing = null;
        break;
      default:
        break; // popup actions are handled by the editor itself
    }
  }

  /** Replace the text of a block outright (the `[[` completion). */
  replaceText(id: string, text: string, caret: number): void {
    const next = ops.setText(this.tree, id, text);
    if (next) this.commit(next, { id, caret });
  }

  undo(): void {
    const prev = this.history.undo(this.snapshot());
    if (prev) this.restore(prev);
  }

  redo(): void {
    const next = this.history.redo(this.snapshot());
    if (next) this.restore(next);
  }

  private restore(s: Snapshot): void {
    this.tree = s.tree;
    this.editing = s.focus && ops.locate(s.tree, s.focus) ? { id: s.focus, caret: "end", nonce: ++this.nonce } : null;
    this.queueSave();
  }

  /** Start a page with a first block: focus the only block of an empty page. */
  focusFirst(): void {
    const first = this.tree[0];
    if (first) this.focus(first.id, "end");
  }
}

function strip(n: BlockNode): BlockNode {
  return { content: n.content, children: n.children.map(strip) };
}
