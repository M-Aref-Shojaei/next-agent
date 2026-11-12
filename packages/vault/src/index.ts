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
