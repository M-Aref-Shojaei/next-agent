// Behaviour follows Logseq (AGPL-3.0): block text conventions — hidden properties
// (`id`, `collapsed`), task markers and the TODO → DOING → DONE cycle
// (frontend/handler/editor.cljs, cycle-todo!).
//
// A block's `content` is its raw text without the bullet: first line, then
// property lines (`key:: value`), then more lines. The editor shows everything
// except the hidden properties and puts them back when the text changes, so a
// block the user edits keeps its `id::` and `collapsed::`.

const HIDDEN = new Set(["id", "collapsed", "heading", "background-color", "created-at", "updated-at"]);
const PROPERTY = /^\s*([A-Za-z0-9_/-]+)::(?:\s+(.*)|\s*)$/;
const MARKER = /^((?:#{1,6}\s+)?)(TODO|DOING|DONE|NOW|LATER|WAITING|CANCELED|CANCELLED|IN-PROGRESS)(?=\s|$)/;

const normalizeKey = (raw: string): string => raw.toLowerCase().replace(/[ _]/g, "-");

export function isHiddenProperty(key: string): boolean {
  const k = normalizeKey(key);
  return HIDDEN.has(k) || k.startsWith("query-") || k.startsWith("hl-") || k === "custom-id";
}

/** `{ key, value }` of a property line, or null. */
export function propertyLine(line: string): { key: string; value: string } | null {
  const m = PROPERTY.exec(line);
  return m ? { key: m[1]!, value: m[2] ?? "" } : null;
}

/** Index just past the title line and the property run that follows it. */
function propertyRunEnd(lines: string[]): number {
  let i = 1;
  while (i < lines.length && propertyLine(lines[i]!)) i++;
  return i;
}

/** The text shown in the editor: hidden property lines removed. */
export function editableText(content: string): string {
  const lines = content.split("\n");
  const end = propertyRunEnd(lines);
  return lines.filter((l, i) => !(i > 0 && i < end && isHidden(l))).join("\n");
}

function isHidden(line: string): boolean {
  const p = propertyLine(line);
  return p !== null && isHiddenProperty(p.key);
}

/** The hidden property lines of `content`, in order. */
function hiddenLines(content: string): string[] {
  const lines = content.split("\n");
  const end = propertyRunEnd(lines);
  return lines.filter((l, i) => i > 0 && i < end && isHidden(l));
}

/**
 * New content for a block whose visible text was edited: the hidden properties
 * go back right after the property run of the new text. Unchanged text returns
 * the original string, so an untouched block keeps its bytes.
 */
export function withEditedText(content: string, text: string): string {
  if (editableText(content) === text) return content;
  const hidden = hiddenLines(content);
  if (hidden.length === 0) return text;
  const lines = text.split("\n");
  const end = propertyRunEnd(lines);
  lines.splice(end, 0, ...hidden);
  return lines.join("\n");
}

// ----------------------------------------------------------- collapsed:: --

export function isCollapsed(content: string): boolean {
  const lines = content.split("\n");
  const end = propertyRunEnd(lines);
  return lines.slice(1, end).some((l) => {
    const p = propertyLine(l);
    return p !== null && normalizeKey(p.key) === "collapsed" && p.value.trim() === "true";
  });
}

export function setCollapsed(content: string, collapsed: boolean): string {
  if (isCollapsed(content) === collapsed) return content;
  const lines = content.split("\n");
  const end = propertyRunEnd(lines);
  if (collapsed) {
    lines.splice(end, 0, "collapsed:: true");
    return lines.join("\n");
  }
  return lines.filter((l, i) => !(i > 0 && i < end && propertyLine(l) && normalizeKey(propertyLine(l)!.key) === "collapsed")).join("\n");
}

// ------------------------------------------------------------------ tasks --

export type Marker = "TODO" | "DOING" | "DONE" | "NOW" | "LATER" | "WAITING" | "CANCELED" | "CANCELLED" | "IN-PROGRESS";

export function markerOf(content: string): Marker | null {
  const m = MARKER.exec(content.split("\n", 1)[0]!);
  return m ? (m[2] as Marker) : null;
}

/** Click on the checkbox: TODO → DOING → DONE → TODO (LATER → NOW → DONE). */
export function nextMarker(marker: Marker): Marker {
  switch (marker) {
    case "TODO":
      return "DOING";
    case "DOING":
    case "IN-PROGRESS":
      return "DONE";
    case "LATER":
      return "NOW";
    case "NOW":
      return "DONE";
    default:
      return "TODO"; // DONE, WAITING, CANCELED
  }
}

/** Replace, add (`null` → `next`) or remove (`next` null) the marker on the first line. */
export function setMarker(content: string, next: Marker | null): string {
  const nl = content.indexOf("\n");
  const first = nl === -1 ? content : content.slice(0, nl);
  const rest = nl === -1 ? "" : content.slice(nl);
  const m = MARKER.exec(first);
  let line: string;
  if (m) {
    const after = first.slice(m[0].length).replace(/^\s/, "");
    line = next ? `${m[1]}${next}${after ? ` ${after}` : ""}` : `${m[1]}${after}`;
  } else {
    const heading = /^(#{1,6}\s+)/.exec(first)?.[1] ?? "";
    const body = first.slice(heading.length);
    line = next ? `${heading}${next}${body ? ` ${body}` : ""}` : first;
  }
  return line + rest;
}

/** Mod+Enter in the editor: none → TODO → DOING → DONE → none. */
export function cycleMarker(content: string): string {
  const current = markerOf(content);
  if (current === null) return setMarker(content, "TODO");
  if (current === "DONE" || current === "CANCELED" || current === "CANCELLED") return setMarker(content, null);
  return setMarker(content, nextMarker(current));
}

// -------------------------------------------------------------- autocomplete --

export interface LinkContext {
  /** Index of the first `[` of the `[[`. */
  start: number;
  query: string;
}

/** Is the caret inside an unclosed `[[…`? */
export function linkContext(text: string, caret: number): LinkContext | null {
  const before = text.slice(0, caret);
  const open = before.lastIndexOf("[[");
  if (open === -1) return null;
  const typed = before.slice(open + 2);
  if (typed.includes("]]") || typed.includes("\n") || typed.includes("[[")) return null;
  return { start: open, query: typed };
}

/** Replace the `[[query` before the caret with `[[name]]` (an existing `]]` after it is reused). */
export function completeLink(text: string, caret: number, ctx: LinkContext, name: string): { text: string; caret: number } {
  const after = text.slice(caret);
  const rest = after.startsWith("]]") ? after.slice(2) : after;
  const head = `${text.slice(0, ctx.start)}[[${name}]]`;
  return { text: head + rest, caret: head.length };
}
