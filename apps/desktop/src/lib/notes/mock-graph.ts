// In-memory notes graph for the browser (vite dev, screenshots). It mirrors the shape of the
// Rust commands, not their full behaviour: no files, a small query evaluator, simple search.
import type { Api } from "../api.ts";
import { dayNumber, type Backlink, type BlockNode, type GraphData, type PageView } from "./types.ts";
import { markerOf } from "./content.ts";
import { tokenize } from "./render.ts";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const ordinal = (n: number) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? "th" : (["th", "st", "nd", "rd"][n % 10] ?? "th")}`;
const journalTitle = (day: number) => `${MONTHS[Math.floor(day / 100) % 100 - 1]} ${ordinal(day % 100)}, ${Math.floor(day / 10000)}`;
const dayOffset = (offset: number) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return dayNumber(d);
};

interface Page {
  name: string;
  journalDay: number | null;
  blocks: BlockNode[];
}
const b = (content: string, ...children: BlockNode[]): BlockNode => ({ content, children });

const today = dayOffset(0);
const pages = new Map<string, Page>();
const put = (name: string, blocks: BlockNode[], journalDay: number | null = null) => pages.set(name.toLowerCase(), { name, blocks, journalDay });

put("Next Agent", [
  b("A local-first AI workstation built on [[Logseq]] notes, #local-first"),
  b("Roadmap", b("TODO [#A] Ship the outliner [[Notes UI]]"), b("DOING Wire search to [[Logseq]]"), b("DONE Notes engine")),
  b("Architecture sketch", b("```mermaid\ngraph TD\n  UI[Svelte UI] --> Core[Rust core]\n  Core --> Index[(SQLite FTS5)]\n  Core --> Files[Markdown files]\n  Files -->|watcher| Index\n```")),
]);
put("Notes UI", [b("Outliner with [[Logseq]] keys: Enter, Tab, Shift+Tab, Alt+Up/Down"), b("Open tasks: {{query (and [[Notes UI]] (task TODO DOING))}}")]);
put("Logseq", [b("Open source outliner, AGPL-3.0"), b("Format we read and write: Markdown with `- ` bullets and `key:: value` properties")]);
put("Garden Redesign", [b("DONE Measure the beds"), b("TODO Order seeds [[Tomatoes]]"), b("TODO Build the compost bin")]);
put("Tomatoes", [b("Plant after the last frost")]);
put(journalTitle(today), [b("Morning: review [[Next Agent]] roadmap"), b("TODO Order seeds [[Tomatoes]]"), b("Idea: graph view for [[Notes UI]] #idea")], today);
put(journalTitle(dayOffset(-1)), [b("DONE Measure the beds [[Garden Redesign]]"), b("Read about local-first software")], dayOffset(-1));
put(journalTitle(dayOffset(-2)), [b("Planned the week")], dayOffset(-2));

const listeners = new Set<(files: string[]) => void>();
const lc = (s: string) => s.trim().toLowerCase();

function walk(blocks: BlockNode[], parents: string[] = []): { block: BlockNode; breadcrumb: string[] }[] {
  return blocks.flatMap((x) => [{ block: x, breadcrumb: parents }, ...walk(x.children, [...parents, x.content.split("\n")[0]!])]);
}
const refsOf = (content: string) => tokenize(content).flatMap((t) => (t.kind === "link" || t.kind === "tag" ? [lc(t.name)] : []));
let nextBlockId = 1;
function rows(pred: (x: BlockNode, page: Page) => boolean, skip?: string): Backlink[] {
  return [...pages.values()]
    .filter((p) => lc(p.name) !== skip)
    .sort((x, y) => (y.journalDay ?? 0) - (x.journalDay ?? 0) || x.name.localeCompare(y.name))
    .flatMap((p) => walk(p.blocks).filter(({ block }) => pred(block, p)).map(({ block, breadcrumb }) => ({ page: p.name, journalDay: p.journalDay, blockId: nextBlockId++, uuid: null, content: block.content, breadcrumb })));
}

function evalQuery(text: string, block: BlockNode, page: Page): boolean {
  const src = text.replace(/^\s*\{\{\s*query\s+|\s*\}\}\s*$/g, "");
  const toks = src.match(/\[\[[^\]]+\]\]|\(|\)|"[^"]*"|[^\s()]+/g) ?? [];
  let i = 0;
  const node = (): boolean => {
    const t = toks[i++]!;
    if (t === "(") {
      const op = toks[i++]!.toLowerCase();
      const args: (boolean | string)[] = [];
      while (toks[i] !== ")" && i < toks.length) {
        if (toks[i] === "(") args.push(node());
        else args.push(toks[i++]!.replace(/^\[\[|\]\]$/g, "").replace(/^"|"$/g, ""));
      }
      i++;
      const bools = args.filter((a): a is boolean => typeof a === "boolean");
      const names = args.filter((a): a is string => typeof a === "string");
      if (op === "and") return bools.every(Boolean);
      if (op === "or") return bools.some(Boolean);
      if (op === "not") return !bools.some(Boolean);
      if (op === "task") return names.map((n) => n.toUpperCase()).includes(markerOf(block.content) ?? "");
      if (op === "page") return lc(names[0] ?? "") === lc(page.name);
      if (op === "priority") return names.some((n) => block.content.includes(`[#${n.toUpperCase()}]`));
      return true;
    }
    if (t.startsWith("[[")) return refsOf(block.content).includes(lc(t.slice(2, -2)));
    return block.content.toLowerCase().includes(t.replace(/^"|"$/g, "").toLowerCase());
  };
  const results: boolean[] = [];
  while (i < toks.length) results.push(node());
  return results.every(Boolean);
}

function view(p: Page): PageView {
  return { name: p.name, file: `${p.journalDay ? "journals" : "pages"}/${p.name}.md`, journalDay: p.journalDay, properties: [], preamble: "", blocks: p.blocks };
}

export const mockGraphApi: Pick<Api, "graphOpen" | "graphSearch" | "graphPage" | "graphBacklinks" | "graphJournals" | "graphSavePage" | "graphPages" | "graphGraph" | "graphQuery" | "graphJournalName" | "graphBlock" | "onGraphChanged"> = {
  async graphOpen(path) {
    return { root: path, pages: pages.size, blocks: [...pages.values()].reduce((n, p) => n + walk(p.blocks).length, 0), createdConfig: false };
  },
  async graphSearch(q, limit = 50) {
    const needle = lc(q);
    if (!needle) return [];
    return rows((x) => x.content.toLowerCase().includes(needle))
      .slice(0, limit)
      .map((r) => {
        const at = r.content.toLowerCase().indexOf(needle);
        const snippet = `${r.content.slice(Math.max(0, at - 30), at)}${r.content.slice(at, at + needle.length)}${r.content.slice(at + needle.length, at + needle.length + 60)}`;
        return { page: r.page, blockId: r.blockId, uuid: null, content: r.content, snippet: snippet.replace(/\n/g, " "), score: 1 };
      });
  },
  async graphPage(name) {
    const p = pages.get(lc(name));
    return p ? view(p) : { name: name.trim(), file: null, journalDay: null, properties: [], preamble: "", blocks: [] };
  },
  async graphBacklinks(name) {
    const n = lc(name);
    return {
      linked: rows((x) => refsOf(x.content).includes(n), n),
      unlinked: rows((x) => x.content.toLowerCase().includes(n) && !refsOf(x.content).includes(n), n),
    };
  },
  async graphJournals(limit = 30) {
    return [...pages.values()]
      .filter((p) => p.journalDay)
      .sort((x, y) => y.journalDay! - x.journalDay!)
      .slice(0, limit)
      .map((p) => ({ name: p.name, file: `journals/${p.name}.md`, day: p.journalDay! }));
  },
  async graphSavePage(name, tree) {
    const old = pages.get(lc(name));
    const page: Page = { name: old?.name ?? name.trim(), journalDay: old?.journalDay ?? null, blocks: structuredClone(tree) };
    pages.set(lc(name), page);
    queueMicrotask(() => listeners.forEach((l) => l([view(page).file!])));
    return view(page);
  },
  async graphPages() {
    return [...pages.values()]
      .map((p) => ({ name: p.name, journalDay: p.journalDay, blocks: walk(p.blocks).length, backlinks: rows((x) => refsOf(x.content).includes(lc(p.name)), lc(p.name)).length }))
      .sort((x, y) => x.name.localeCompare(y.name));
  },
  async graphGraph() {
    const nodes = new Map<string, GraphData["nodes"][number]>();
    const edges: GraphData["edges"] = [];
    for (const p of pages.values()) nodes.set(lc(p.name), { id: lc(p.name), name: p.name, journal: p.journalDay !== null, missing: false, degree: 0 });
    for (const p of pages.values()) {
      for (const dst of new Set(walk(p.blocks).flatMap(({ block }) => refsOf(block.content)))) {
        if (dst === lc(p.name)) continue;
        if (!nodes.has(dst)) nodes.set(dst, { id: dst, name: dst, journal: false, missing: true, degree: 0 });
        nodes.get(dst)!.degree++;
        nodes.get(lc(p.name))!.degree++;
        edges.push({ source: lc(p.name), target: dst });
      }
    }
    return { nodes: [...nodes.values()], edges };
  },
  async graphQuery(text) {
    return rows((x, p) => evalQuery(text, x, p)).filter((r) => !r.content.includes("{{query"));
  },
  async graphBlock() {
    return null;
  },
  async graphJournalName(day) {
    return journalTitle(day);
  },
  async onGraphChanged(callback) {
    listeners.add(callback);
    return () => void listeners.delete(callback);
  },
};
