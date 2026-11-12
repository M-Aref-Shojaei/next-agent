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

/** `Tasks/2026/11/2026-11-10-slug.md` for a given date and title. */
export function taskRecordPath(date: Date, title: string): string {
  const year = String(date.getFullYear());
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `Tasks/${year}/${month}/${year}-${month}-${day}-${slugify(title)}.md`;
}

/** Everything below this line in a task record belongs to the user. */
export const TASK_END_MARKER = "<!-- next-agent:end -->";

function isoDateParts(iso: string): [string, string, string] | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  return match ? [match[1], match[2], match[3]] : null;
}

/**
 * Same as {@link taskRecordPath} but for an ISO-8601 timestamp such as
 * `2026-11-10T10:09:40+03:30`. The calendar date written in the string is
 * used as-is (never shifted to another timezone), matching the Rust writer
 * in `crates/core/src/vault.rs`.
 */
export function taskRecordPathFromIso(iso: string, title: string): string | null {
  const parts = isoDateParts(iso);
  if (!parts) return null;
  const [y, m, d] = parts;
  return `Tasks/${y}/${m}/${y}-${m}-${d}-${slugify(title)}.md`;
}

/** `Journal/2026-11-10.md` for an ISO-8601 timestamp. */
export function journalPath(iso: string): string | null {
  const parts = isoDateParts(iso);
  return parts ? `Journal/${parts.join("-")}.md` : null;
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
  const heading = /^# (.+)$/m.exec(body);
  const at = body.indexOf(TASK_END_MARKER);
  return {
    frontmatter: data,
    title: heading ? heading[1].trim() : "",
    links: extractWikilinks(body),
    userNotes: at === -1 ? null : body.slice(at + TASK_END_MARKER.length).replace(/^\r?\n/, ""),
  };
}

/** Journal bullets that link to task records: `- [[stem]] — done (auto)`. */
export function parseJournalLinks(
  source: string,
): { stem: string; status: string; mode: string }[] {
  const out: { stem: string; status: string; mode: string }[] = [];
  for (const line of source.split(/\r?\n/)) {
    const match = /^- \[\[([^\]]+)\]\] — (\S+) \(([^)]*)\)\s*$/.exec(line);
    if (match) out.push({ stem: match[1], status: match[2], mode: match[3] });
  }
  return out;
}
