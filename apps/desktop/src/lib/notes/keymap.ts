// Behaviour follows Logseq (AGPL-3.0): editor keybindings, frontend/modules/shortcut/config.cljs
// (editor/new-block Enter, editor/indent-outdent Tab, editor/move-block-up/down Alt+Up/Down,
// editor/toggle-undo-redo-mode, editor/cycle-todo Mod+Enter, editor/collapse-block Mod+Up).

export type EditorAction =
  | "split"
  | "indent"
  | "outdent"
  | "moveUp"
  | "moveDown"
  | "mergePrevious"
  | "focusPrevious"
  | "focusNext"
  | "undo"
  | "redo"
  | "cycleTask"
  | "collapse"
  | "expand"
  | "exit"
  | "popupAccept"
  | "popupNext"
  | "popupPrevious"
  | "popupClose";

export interface KeyInput {
  key: string;
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  isComposing?: boolean;
}

export interface KeyContext {
  selectionStart: number;
  selectionEnd: number;
  /** The caret is on the first / last visual line of the text. */
  onFirstLine: boolean;
  onLastLine: boolean;
  /** A `[[` autocomplete list is open. */
  popupOpen: boolean;
}

/** What a key press in a block editor means, or null to leave it to the textarea. */
export function editorAction(e: KeyInput, ctx: KeyContext): EditorAction | null {
  if (e.isComposing) return null; // IME in progress: Enter confirms the composition
  const mod = e.metaKey || e.ctrlKey;
  const collapsedSelection = ctx.selectionStart === ctx.selectionEnd;

  if (ctx.popupOpen && !mod && !e.altKey) {
    if (e.key === "Enter" || e.key === "Tab") return "popupAccept";
    if (e.key === "ArrowDown") return "popupNext";
    if (e.key === "ArrowUp") return "popupPrevious";
    if (e.key === "Escape") return "popupClose";
  }

  const k = e.key.toLowerCase();
  if (mod && !e.altKey && k === "z") return e.shiftKey ? "redo" : "undo";
  if (mod && !e.altKey && !e.shiftKey && k === "y") return "redo";
  if (mod && !e.altKey && !e.shiftKey && e.key === "Enter") return "cycleTask";
  if (mod && !e.altKey && !e.shiftKey && e.key === "ArrowUp") return "collapse";
  if (mod && !e.altKey && !e.shiftKey && e.key === "ArrowDown") return "expand";
  if (mod) return null;

  if (e.altKey && !e.shiftKey && e.key === "ArrowUp") return "moveUp";
  if (e.altKey && !e.shiftKey && e.key === "ArrowDown") return "moveDown";
  if (e.altKey) return null;

  if (e.key === "Enter" && !e.shiftKey) return "split";
  if (e.key === "Tab") return e.shiftKey ? "outdent" : "indent";
  if (e.key === "Escape") return "exit";
  if (e.key === "Backspace" && collapsedSelection && ctx.selectionStart === 0 && !e.shiftKey) return "mergePrevious";
  if (!e.shiftKey && e.key === "ArrowUp" && collapsedSelection && ctx.onFirstLine) return "focusPrevious";
  if (!e.shiftKey && e.key === "ArrowDown" && collapsedSelection && ctx.onLastLine) return "focusNext";
  return null;
}

/** Is the caret on the first / last line of `text`? */
export function caretLines(text: string, caret: number): { onFirstLine: boolean; onLastLine: boolean } {
  return {
    onFirstLine: !text.slice(0, caret).includes("\n"),
    onLastLine: !text.slice(caret).includes("\n"),
  };
}
