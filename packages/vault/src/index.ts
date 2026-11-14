export interface Frontmatter {
  data: Record<string, string | string[]>;
  body: string;
}

/** Parses `---` YAML-ish frontmatter. Only flat `key: value` and inline lists. */
export function parseFrontmatter(source: string): Frontmatter {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(source);
  if (!match) return { data: {}, body: source };

  const data: Record<string, string | string[]> = {};
  for (const line of match[1].split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const index = trimmed.indexOf(":");
    if (index <= 0) continue;
    const key = trimmed.slice(0, index).trim();
    const raw = trimmed.slice(index + 1).trim();
    data[key] = raw.startsWith("[")
      ? raw
          .slice(1, raw.lastIndexOf("]"))
          .split(",")
          .map((item) => item.trim().replace(/^["']|["']$/g, ""))
          .filter(Boolean)
      : raw.replace(/^["']|["']$/g, "");
  }
  return { data, body: source.slice(match[0].length) };
}

/** Returns the target of every `[[wikilink]]`, without alias or anchor. */
export function extractWikilinks(source: string): string[] {
  const links: string[] = [];
  for (const match of source.matchAll(/\[\[([^\]\n]+)\]\]/g)) {
    const target = match[1].split("|")[0].split("#")[0].trim();
    if (target) links.push(target);
  }
  return links;
}

/** Converts a title into a lowercase, hyphenated file slug. */
export function slugify(title: string): string {
  return title
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80) || "untitled";
}

/** Everything below this line in a task record belongs to the user. */
export const TASK_END_MARKER = "<!-- next-agent:end -->";

function isoDateParts(iso: string): [string, string, string] | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  return match ? [match[1], match[2], match[3]] : null;
}

// Format follows Logseq (AGPL-3.0, © Logseq contributors) 0.10.9 @ c67b8b5:
// src/main/frontend/util/fs.cljs `tri-lb-file-name-sanity` and
// deps/graph-parser/src/logseq/graph_parser/util.cljs `tri-lb-title-parsing`.
// Mirrors crates/graph/src/layout.rs; fixtures/vault/page-file-names.tsv is
// checked by both.

const RESERVED = /[:*?"<>|#\\]/u;
const WINDOWS_RESERVED = new Set([
  "CON", "PRN", "AUX", "NUL",
  "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
  "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
]);

/** File body (no extension) for a Logseq page title: `/` becomes `___`. */
export function pageFileBody(title: string): string {
  let t = title;
  if (t.startsWith("/")) t = t.slice(1);
  if (t.endsWith("/")) t = t.slice(0, -1);
  let s = "";
  for (let i = 0; i < t.length; ) {
    const ch = String.fromCodePoint(t.codePointAt(i) ?? 0);
    if (ch === "%" && /^%[0-9a-fA-F]{2}/.test(t.slice(i))) s += "%25";
    else if (RESERVED.test(ch)) s += `%${ch.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`;
    else s += ch;
    i += ch.length;
  }
  if (s.startsWith(".")) s = `%2E${s.slice(1)}`;
  if (WINDOWS_RESERVED.has(s) || s.endsWith(".")) s += "/";
  return s
    .replaceAll("___", "%5F%5F%5F")
    .replaceAll("_/", "%5F/")
    .replaceAll("/_", "/%5F")
    .replaceAll("/", "___");
}

/** `<body>.md` for a page title. */
export function pageFileName(title: string): string {
  return `${pageFileBody(title)}.md`;
}

/** Page title for a `:triple-lowbar` file body. */
export function titleFromFileBody(body: string): string {
  const decoded = body
    .replaceAll("___", "/")
    .replace(/%[0-9a-fA-F]{2}/g, (m) => {
      const code = Number.parseInt(m.slice(1), 16);
      return code < 0x80 ? String.fromCharCode(code) : m;
    });
  return decoded.split("/").filter(Boolean).join("/");
}


/**
 * `2026-11-10-slug` for an ISO-8601 timestamp such as
 * `2026-11-10T10:09:40+03:30`. The calendar date written in the string is
 * used as-is (never shifted to another timezone), matching the Rust writer
 * in `crates/core/src/vault.rs`.
 */
export function taskRecordStem(iso: string, title: string): string | null {
  const parts = isoDateParts(iso);
  return parts ? `${parts.join("-")}-${slugify(title)}` : null;
}

/** Logseq page name of a task record: `Next Agent/<workspace>/Tasks/<stem>`. */
export function taskPageTitle(workspace: string, stem: string): string {
  return `Next Agent/${workspaceFolder(workspace)}/Tasks/${stem}`;
}

/** `pages/Next Agent___<workspace>___Tasks___<stem>.md`, vault-relative. */
export function taskRecordPath(iso: string, workspace: string, title: string): string | null {
  const stem = taskRecordStem(iso, title);
  return stem ? `pages/${pageFileName(taskPageTitle(workspace, stem))}` : null;
}

/**
 * `journals/2026_11_10.md` for an ISO-8601 timestamp. `format` is the
 * vault's `:journal/file-name-format` (numeric `yyyy`, `MM`, `dd` tokens).
 */
export function journalPath(iso: string, format = "yyyy_MM_dd"): string | null {
  const parts = isoDateParts(iso);
  if (!parts) return null;
  const [y, m, d] = parts;
  const name = format
    .replace(/yyyy/g, y)
    .replace(/MM/g, m)
    .replace(/dd/g, d);
  return `journals/${name}.md`;
}

/** Folder name used for a workspace under `Next Agent/`. */
export function workspaceFolder(name: string): string {
  const cleaned = name
    .replace(/[^\p{L}\p{N} ._-]/gu, "-")
    .replace(/^[. -]+|[. -]+$/g, "");
  return cleaned || "workspace";
}

export interface TaskRecord {
  frontmatter: Frontmatter["data"];
  title: string;
  /** `[[links]]` found anywhere in the record. */
  links: string[];
  /** Text after the end marker, or null when the marker is missing. */
  userNotes: string | null;
}

/** Parses a task record written by the Rust vault writer. */
export function parseTaskRecord(source: string): TaskRecord {
  const { data, body } = parseFrontmatter(source);
  const heading = /^- # (.+)$/m.exec(body) ?? /^# (.+)$/m.exec(body);
  const at = body.indexOf(TASK_END_MARKER);
  return {
    frontmatter: data,
    title: heading ? heading[1].trim() : "",
    links: extractWikilinks(body),
    userNotes:
      at === -1
        ? null
        : body
            .slice(at + TASK_END_MARKER.length)
            .replace(/^\r?\n/, "")
            .replace(/^ {2}/, ""),
  };
}

/** Journal block linking a task record page. */
export function journalLine(page: string, status: string, mode: string): string {
  return `- [[${page}]] — ${status} (${mode})`;
}

/** Journal blocks that link to task records: `- [[page]] — done (auto)`. */
export function parseJournalLinks(
  source: string,
): { page: string; status: string; mode: string }[] {
  const out: { page: string; status: string; mode: string }[] = [];
  for (const line of source.split(/\r?\n/)) {
    const match = /^- \[\[([^\]]+)\]\] — (\S+) \(([^)]*)\)\s*$/.exec(line);
    if (match) out.push({ page: match[1], status: match[2], mode: match[3] });
  }
  return out;
}
