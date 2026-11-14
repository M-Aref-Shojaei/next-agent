// Wire types of the notes-engine Tauri commands (apps/desktop/src-tauri/src/notes.rs,
// crates/graph). Keep in step with the Rust structs; see docs/architecture/Notes Engine.md.

export interface Property {
  /** Normalised key: lowercase, spaces and `_` replaced by `-`. */
  key: string;
  rawKey: string;
  value: string;
}

export type RefKind = "link" | "tag" | "property" | "embed" | "block_ref";

export type Ref =
  | { type: "page"; name: string; kind: RefKind }
  | { type: "block"; uuid: string; kind: RefKind };

/** One outliner block. Only `content` and `children` are read on save. */
export interface BlockNode {
  content: string;
  children: BlockNode[];
  uuid?: string;
  marker?: string;
  priority?: string;
  scheduled?: string;
  deadline?: string;
  collapsed?: boolean;
  properties?: Property[];
  refs?: Ref[];
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

export interface IndexStats {
  indexed: number;
  removed: number;
  unchanged: number;
  conflicts: string[];
}

/** `graph_open({ path, create? })`: create defaults to false (see Notes Engine.md). */
export interface GraphInfo {
  root: string;
  pages: number;
  blocks: number;
  createdConfig: boolean;
  stats: IndexStats;
}

/** Search snippet match markers (Unicode private use; split, don't render as HTML). */
export const MATCH_START = "";
export const MATCH_END = "";

export interface SearchHit {
  page: string;
  blockId: number;
  uuid: string | null;
  content: string;
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

export interface JournalEntry {
  name: string;
  file: string;
  day: number;
}

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
