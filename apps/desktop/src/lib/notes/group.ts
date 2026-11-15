import type { Backlink } from "./types.ts";

/** Rows grouped by page, in first-seen order. */
export function groupByPage(rows: readonly Backlink[]): [string, Backlink[]][] {
  const groups = new Map<string, Backlink[]>();
  for (const r of rows) {
    const list = groups.get(r.page);
    if (list) list.push(r);
    else groups.set(r.page, [r]);
  }
  return [...groups];
}
