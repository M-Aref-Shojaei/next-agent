// Wire types of the notes-engine Tauri commands (apps/desktop/src-tauri/src/notes.rs).
// Same shapes as packages/vault/src/graph.ts, plus the Notes UI commands.

export interface Property {
  key: string;
  rawKey: string;
  value: string;
}

/** One outliner block as the engine sends it. Only `content` and `children` are read on save. */
export interface BlockNode {
  content: string;
  children: BlockNode[];
  uuid?: string;
  marker?: string;
  priority?: string;
  collapsed?: boolean;
  properties?: Property[];
}

export interface PageView {
  name: string;
  /** Graph-relative file; null for a page that is only referenced. */
  file: string | null;
  /** `yyyyMMdd` for journal pages. */
  journalDay: number | null;
  properties: Property[];
  /** Text before the first block (frontmatter, page properties), verbatim. */
  preamble: string;
  blocks: BlockNode[];
}

export interface GraphInfo {
  root: string;
  pages: number;
  blocks: number;
  createdConfig: boolean;
}

export interface SearchHit {
  page: string;
  blockId: number;
  uuid: string | null;
  content: string;
  /** Matches are wrapped in U+E000 / U+E001. */
  snippet: string;
  score: number;
}

export interface Backlink {
  page: string;
  journalDay: number | null;
  blockId: number;
  uuid: string | null;
  content: string;
  /** First lines of ancestor blocks, outermost first. */
  breadcrumb: string[];
}

export interface Backlinks {
  linked: Backlink[];
  unlinked: Backlink[];
}

/** The block a `((uuid))` points at. */
export interface BlockRef {
  page: string;
  content: string;
}

export interface JournalEntry {
  name: string;
  file: string;
  day: number;
}

export interface PageListItem {
  name: string;
  journalDay: number | null;
  blocks: number;
  backlinks: number;
}

export interface GraphNode {
  id: string;
  name: string;
  journal: boolean;
  missing: boolean;
  degree: number;
}
export interface GraphEdge {
  source: string;
  target: string;
}
export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

const MATCH_START = "";
const MATCH_END = "";

/** Splits a search snippet into plain and matched parts. */
export function snippetParts(snippet: string): { text: string; match: boolean }[] {
  const parts: { text: string; match: boolean }[] = [];
  for (const [i, chunk] of snippet.split(MATCH_START).entries()) {
    if (i === 0) {
      if (chunk) parts.push({ text: chunk, match: false });
      continue;
    }
    const end = chunk.indexOf(MATCH_END);
    const hit = end === -1 ? chunk : chunk.slice(0, end);
    const rest = end === -1 ? "" : chunk.slice(end + MATCH_END.length);
    if (hit) parts.push({ text: hit, match: true });
    if (rest) parts.push({ text: rest, match: false });
  }
  return parts;
}

/** `yyyyMMdd` of a local date. */
export function dayNumber(date: Date): number {
  return date.getFullYear() * 10000 + (date.getMonth() + 1) * 100 + date.getDate();
}
