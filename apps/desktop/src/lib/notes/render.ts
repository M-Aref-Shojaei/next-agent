// Behaviour follows Logseq (AGPL-3.0): how a block's text is read for display —
// page refs, tags, block refs, task markers, priority, property drawers and macros
// (deps/graph-parser/src/logseq/graph_parser/block.cljs, mldoc inline syntax).
//
// Everything is turned into plain data; Svelte renders it without `{@html}`.

import { isHiddenProperty, propertyLine, type Marker } from "./content.ts";

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "link"; name: string; label: string }
  | { kind: "tag"; name: string }
  | { kind: "blockref"; uuid: string }
  | { kind: "code"; text: string }
  | { kind: "bold"; text: string }
  | { kind: "italic"; text: string }
  | { kind: "url"; href: string; label: string };

// One pass, leftmost match wins, so a `#tag` inside `code` or a URL stays text.
const INLINE = new RegExp(
  [
    String.raw`(?<code>\x60[^\x60\n]+\x60)`,
    String.raw`(?<mdlink>\[(?<mdlabel>[^\]\n]*)\]\((?<mdurl>https?:\/\/[^\s)]+)\))`,
    String.raw`(?<link>\[\[(?<linkname>[^\]\n]+)\]\])`,
    String.raw`(?<blockref>\(\((?<uuid>[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\)\))`,
    String.raw`(?<url>https?:\/\/[^\s<>\x60]+[^\s<>\x60.,;:!?)])`,
    String.raw`(?<tagbr>(?<=^|\s)#\[\[(?<tagbrname>[^\]\n]+)\]\])`,
    String.raw`(?<tag>(?<=^|[\s(])#(?<tagname>[^\s#,.!?;:()\[\]{}"'\x60]+))`,
    String.raw`(?<bold>\*\*(?<boldtext>[^*\n]+)\*\*)`,
    String.raw`(?<italic>(?<![*\w])\*(?<italictext>[^*\s][^*\n]*)\*(?![*\w]))`,
  ].join("|"),
  "gu",
);

export function tokenize(text: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  const push = (t: Inline) => out.push(t);
  for (const m of text.matchAll(INLINE)) {
    const g = m.groups!;
    if (m.index > last) push({ kind: "text", text: text.slice(last, m.index) });
    last = m.index + m[0].length;
    if (g.code) push({ kind: "code", text: g.code.slice(1, -1) });
    else if (g.mdlink) push({ kind: "url", href: g.mdurl!, label: g.mdlabel || g.mdurl! });
    else if (g.link) push({ kind: "link", name: g.linkname!, label: g.linkname! });
    else if (g.blockref) push({ kind: "blockref", uuid: g.uuid!.toLowerCase() });
    else if (g.url) push({ kind: "url", href: g.url, label: g.url });
    else if (g.tagbr) push({ kind: "tag", name: g.tagbrname! });
    else if (g.tag) push({ kind: "tag", name: g.tagname! });
    else if (g.bold) push({ kind: "bold", text: g.boldtext! });
    else push({ kind: "italic", text: g.italictext! });
  }
  if (last < text.length) push({ kind: "text", text: text.slice(last) });
  return out;
}

export type Part =
  | { kind: "line"; inline: Inline[] }
  | { kind: "code"; lang: string; code: string }
  | { kind: "property"; key: string; value: Inline[] }
  | { kind: "meta"; label: string; value: string }
  | { kind: "query"; text: string };

export interface Body {
  marker: Marker | null;
  /** `A`, `B` or `C` from `[#A]`. */
  priority: string | null;
  /** 1–6 for `# Heading` blocks. */
  heading: number | null;
  parts: Part[];
}

const MARKER_LINE = /^(?:(#{1,6})\s+)?(TODO|DOING|DONE|NOW|LATER|WAITING|CANCELED|CANCELLED|IN-PROGRESS)(?:\s+|$)/;
const PRIORITY = /^\[#([A-C])\]\s*/;
const FENCE = /^\s*(```|~~~)\s*([\w+-]*)/;
const QUERY = /\{\{\s*query\s+(.*?)\s*\}\}/i;

/** The first `{{query …}}` macro in a line: its text and the text around it. */
function queryIn(line: string): { text: string; before: string; after: string } | null {
  const m = QUERY.exec(line);
  const text = m?.[1]?.trim();
  return m && text ? { text, before: line.slice(0, m.index), after: line.slice(m.index + m[0].length) } : null;
}

/** Pull the query text out of a `{{query …}}` line, or null. */
export const queryOf = (line: string): string | null => queryIn(line)?.text ?? null;

export function parseBody(content: string): Body {
  const lines = content.split("\n");
  let first = lines[0] ?? "";
  let heading: number | null = null;
  let marker: Marker | null = null;
  let priority: string | null = null;

  const m = MARKER_LINE.exec(first);
  if (m) {
    marker = m[2] as Marker;
    if (m[1]) heading = m[1].length;
    first = first.slice(m[0].length);
  } else {
    const h = /^(#{1,6})\s+/.exec(first);
    if (h) {
      heading = h[1]!.length;
      first = first.slice(h[0].length);
    }
  }
  const p = PRIORITY.exec(first);
  if (p) {
    priority = p[1]!;
    first = first.slice(p[0].length);
  }

  const parts: Part[] = [];
  const addLine = (line: string) => {
    const q = queryIn(line);
    if (!q) return void parts.push({ kind: "line", inline: tokenize(line) });
    if (q.before.trim()) parts.push({ kind: "line", inline: tokenize(q.before.trimEnd()) });
    parts.push({ kind: "query", text: q.text });
    if (q.after.trim()) parts.push({ kind: "line", inline: tokenize(q.after.trimStart()) });
  };
  lines[0] = first;
  let inProps = false; // property lines count only right after the title line
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    const fence = FENCE.exec(line);
    if (fence) {
      inProps = false;
      const body: string[] = [];
      i++;
      // An unclosed fence runs to the end of the block, as the writer keeps it.
      while (i < lines.length && !lines[i]!.trim().startsWith(fence[1]!)) body.push(lines[i++]!);
      i++;
      parts.push({ kind: "code", lang: fence[2]!.toLowerCase(), code: body.join("\n") });
      continue;
    }
    const at = i++;
    if (at === 0) {
      inProps = true;
      if (line !== "" || lines.length === 1) addLine(line);
      continue;
    }
    const prop = inProps ? propertyLine(line) : null;
    if (prop) {
      if (!isHiddenProperty(prop.key) && prop.value !== "") parts.push({ kind: "property", key: prop.key, value: tokenize(prop.value) });
      continue;
    }
    inProps = false;
    const meta = /^\s*(SCHEDULED|DEADLINE|CLOSED):\s*(.*)$/.exec(line);
    if (meta) {
      parts.push({ kind: "meta", label: meta[1]!, value: meta[2]! });
      continue;
    }
    if (/^\s*:LOGBOOK:\s*$/.test(line)) {
      while (i < lines.length && !/^\s*:END:\s*$/.test(lines[i]!)) i++;
      i++;
      continue;
    }
    addLine(line);
  }
  return { marker, priority, heading, parts };
}

/** `true` when a `\`\`\`mermaid` fence is present (for lazy-loading the renderer). */
export const hasMermaid = (body: Body): boolean => body.parts.some((p) => p.kind === "code" && p.lang === "mermaid");
