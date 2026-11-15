// Key-capture logic for <ShortcutField> and <Kbd>. Pure functions so they can
// be tested without a DOM (apps/desktop/tests/ds-shortcut.test.ts).
//
// A shortcut is stored as a canonical string: modifiers in a fixed order
// (Ctrl, Alt, Shift, Meta) then one key, joined by "+", e.g. "Ctrl+Shift+K".
// The shape of `Recorded` mirrors Coucou's recorder (core/shortcuts.ts) so its
// settings can move onto <ShortcutField> by passing its own `record`.

/** The parts of a KeyboardEvent the recorder reads. */
export interface KeyPress {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type Recorded =
  /** Only modifiers so far: keep listening. */
  | { kind: "pending" }
  | { kind: "cancel" }
  | { kind: "clear" }
  | { kind: "keys"; keys: string }
  /** A key with no Ctrl/Alt/Meta, when a modifier is required. */
  | { kind: "needsModifier" }
  | { kind: "unsupported" };

export type Platform = "mac" | "other";

const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta", "AltGraph", "OS", "Hyper", "Super", "CapsLock", "Fn"]);
const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"] as const;
type Modifier = (typeof MODIFIERS)[number];

const NAMED: Record<string, string> = {
  " ": "Space", Spacebar: "Space", Esc: "Escape",
  ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
};
const SUPPORTED_NAMED = new Set([
  "Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
  "Up", "Down", "Left", "Right",
]);

/** The key name a press stands for, independent of Shift/Alt producing another character. */
export function pressedKey(e: Pick<KeyPress, "key" | "code">): string | null {
  // Physical letter and digit keys: use the code, so Shift+1 is "1" and Alt+K on a Mac is "K", not "˚".
  const letter = /^Key([A-Z])$/.exec(e.code);
  if (letter) return letter[1];
  const digit = /^(?:Digit|Numpad)([0-9])$/.exec(e.code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) return e.key;
  const named = NAMED[e.key] ?? e.key;
  if (SUPPORTED_NAMED.has(named)) return named;
  if ([...e.key].length === 1 && /[\p{L}\p{N}\p{P}\p{S}]/u.test(e.key)) return e.key.toUpperCase();
  return null;
}

/** Canonical string for a set of modifiers and a key. */
export function formatKeys(mods: Partial<Record<Modifier, boolean>>, key: string): string {
  return [...MODIFIERS.filter((m) => mods[m]), key].join("+");
}

/** Turns one keydown into a recorder result. */
export function recordPress(e: KeyPress, { requireModifier = true } = {}): Recorded {
  if (MODIFIER_KEYS.has(e.key)) return { kind: "pending" };
  const bare = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
  if (e.key === "Escape" && bare) return { kind: "cancel" };
  if ((e.key === "Backspace" || e.key === "Delete") && bare) return { kind: "clear" };
  const key = pressedKey(e);
  if (!key) return { kind: "unsupported" };
  if (requireModifier && !(e.ctrlKey || e.altKey || e.metaKey)) return { kind: "needsModifier" };
  return {
    kind: "keys",
    keys: formatKeys({ Ctrl: e.ctrlKey, Alt: e.altKey, Shift: e.shiftKey, Meta: e.metaKey }, key),
  };
}

const MAC_GLYPHS: Record<string, string> = {
  Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘",
  Enter: "↩", Backspace: "⌫", Delete: "⌦", Escape: "⎋", Tab: "⇥", Space: "Space",
  Up: "↑", Down: "↓", Left: "←", Right: "→", PageUp: "⇞", PageDown: "⇟", Home: "↖", End: "↘",
};
const OTHER_NAMES: Record<string, string> = {
  Meta: "Win", Escape: "Esc", Up: "↑", Down: "↓", Left: "←", Right: "→",
};

/** Splits a canonical shortcut into display keycaps for a platform. Empty for "". */
export function shortcutParts(keys: string, platform: Platform = "other"): string[] {
  const names = platform === "mac" ? MAC_GLYPHS : OTHER_NAMES;
  return splitKeys(keys).map((p) => names[p] ?? p);
}

function splitKeys(keys: string): string[] {
  if (!keys.trim()) return [];
  // "Ctrl++" means Ctrl and the plus key.
  const parts = keys.endsWith("++") ? [...keys.slice(0, -2).split("+"), "+"] : keys.split("+");
  return parts.filter(Boolean);
}

const MAC_WORDS: Record<string, string> = { Ctrl: "Control", Alt: "Option", Meta: "Command" };

/** Words for screen readers and tooltips, e.g. "Command+Shift+K" on a Mac, "Ctrl+Shift+K" elsewhere. */
export function shortcutLabel(keys: string, platform: Platform = "other"): string {
  const words: Record<string, string> = platform === "mac" ? MAC_WORDS : { Meta: "Win" };
  return splitKeys(keys).map((p) => words[p] ?? p).join("+");
}

/** Best guess at the current platform, for display only. */
export function currentPlatform(): Platform {
  const nav = typeof navigator === "undefined" ? undefined : navigator;
  return nav && /Mac|iPhone|iPad/.test(nav.platform || nav.userAgent) ? "mac" : "other";
}
