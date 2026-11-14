// Design Studio: types and pure helpers (no Svelte, no Tauri) so they can be
// tested with node:test. Rust counterparts: crates/design, src-tauri/src/design.rs.

export type FrameKind = "web" | "mobile" | "desktop";
export type Framework = "svelte" | "react" | "vue" | "web-component";

/** Must match `crates/design/src/sandbox.rs` (checked by tests/design.test.ts). */
export const DIRECTION_SANDBOX = "";
export const APP_SANDBOX = "allow-scripts allow-same-origin allow-forms";

export const FRAMES: Record<FrameKind, { label: string; width: number; height: number }> = {
  web: { label: "Web", width: 1280, height: 800 },
  mobile: { label: "Mobile", width: 390, height: 844 },
  desktop: { label: "Desktop", width: 1440, height: 900 },
};

export const FRAMEWORKS: { id: Framework; label: string }[] = [
  { id: "svelte", label: "Svelte" },
  { id: "react", label: "React" },
  { id: "vue", label: "Vue" },
  { id: "web-component", label: "Web Component" },
];

export interface DesignToken {
  name: string;
  value: string;
  dark?: string;
  kind: "color" | "font" | "size" | "spacing" | "radius" | "shadow" | "other";
  derived: boolean;
}
export interface DesignFiles {
  designPath: string;
  tokensPath: string;
  designMd: string;
  tokensCss: string;
  saved: boolean;
  tokens: DesignToken[];
  sources: string[];
}
export interface Critique {
  scores: [string, number][];
  notes: string;
  revised: boolean;
}
export interface LintFinding {
  rule: string;
  severity: "p0" | "p1" | "p2";
  message: string;
}
export interface DesignDirection {
  id: string;
  batchId: string;
  parentId: string | null;
  prompt: string;
  frame: FrameKind;
  identifier: string;
  title: string;
  rationale: string;
  html: string;
  srcdoc: string;
  critique: Critique | null;
  lint: LintFinding[];
  note: string;
  reply: string;
  chosen: boolean;
  createdAt: string;
}
export interface ModelInfo {
  label: string;
  mock: boolean;
  reason: string | null;
}
export interface DesignBatch {
  batchId: string;
  directions: DesignDirection[];
  model: ModelInfo;
  notices: string[];
}
export type DesignProgress =
  | { phase: "generating"; count: number }
  | { phase: "critiquing"; index: number; total: number; title: string }
  | { phase: "done" };
export interface PreviewInfo {
  url: string;
  origin: string;
  target: string;
  command: string | null;
  sandbox: string;
}
export interface RawLocation {
  origin: "attr" | "svelte" | "svelte4" | "react" | "vue";
  file: string;
  line: number;
  column: number;
  tag: string;
}
export interface SourceView {
  file: string;
  line: number;
  column: number;
  tag: string;
  firstLine: number;
  lines: string[];
  syntax: "svelte" | "vue" | "html" | "jsx" | null;
}
export interface StyleChange {
  property: string;
  value: string;
}
export interface DesignTask {
  taskId: string;
  title: string;
  path: string | null;
}

/** Scale that fits a frame into `available` px of width (never upscales). */
export function fitScale(frame: FrameKind, available: number): number {
  if (!(available > 0)) return 0.1;
  return Math.min(1, available / FRAMES[frame].width);
}

export function progressLabel(p: DesignProgress | null): string {
  if (!p) return "";
  if (p.phase === "generating") return `Designing ${p.count} directions…`;
  if (p.phase === "critiquing") return `Self-critique ${p.index + 1}/${p.total}: ${p.title}`;
  return "Done";
}

/** Average critique score (1–5), or null. */
export function averageScore(c: Critique | null): number | null {
  if (!c || c.scores.length === 0) return null;
  return Math.round((c.scores.reduce((n, [, s]) => n + s, 0) / c.scores.length) * 10) / 10;
}

/** The lineage of a direction, oldest first (`all` may hold several batches). */
export function lineage(all: DesignDirection[], id: string): DesignDirection[] {
  const byId = new Map(all.map((d) => [d.id, d]));
  const out: DesignDirection[] = [];
  for (let d = byId.get(id); d && out.length < 50; d = d.parentId ? byId.get(d.parentId) : undefined) out.unshift(d);
  return out;
}

/** Latest version per original direction for a batch (refinements replace their parents). */
export function batchHeads(all: DesignDirection[], batchId: string): DesignDirection[] {
  const inBatch = all.filter((d) => d.batchId === batchId);
  const parents = new Set(inBatch.map((d) => d.parentId).filter(Boolean));
  return inBatch.filter((d) => !parents.has(d.id)).sort((a, b) => rootOf(inBatch, a).localeCompare(rootOf(inBatch, b)));
}
function rootOf(all: DesignDirection[], d: DesignDirection): string {
  const line = lineage(all, d.id);
  return line[0]?.id ?? d.id;
}

// ---- Edit mode: style inspector ----

export interface StyleField {
  key: string; // CSS property (kebab-case)
  label: string;
  group: "Layout" | "Spacing" | "Type" | "Fill";
  options?: string[];
  tokenKinds?: DesignToken["kind"][];
}

export const STYLE_FIELDS: StyleField[] = [
  { key: "display", label: "Display", group: "Layout", options: ["", "block", "flex", "grid", "inline-flex", "none"] },
  { key: "flex-direction", label: "Direction", group: "Layout", options: ["", "row", "column"] },
  { key: "justify-content", label: "Justify", group: "Layout", options: ["", "flex-start", "center", "flex-end", "space-between"] },
  { key: "align-items", label: "Align", group: "Layout", options: ["", "stretch", "flex-start", "center", "flex-end"] },
  { key: "gap", label: "Gap", group: "Layout", tokenKinds: ["spacing"] },
  { key: "width", label: "Width", group: "Layout" },
  { key: "padding", label: "Padding", group: "Spacing", tokenKinds: ["spacing"] },
  { key: "margin", label: "Margin", group: "Spacing", tokenKinds: ["spacing"] },
  { key: "font-size", label: "Size", group: "Type", tokenKinds: ["size"] },
  { key: "font-weight", label: "Weight", group: "Type", options: ["", "400", "500", "600", "700"] },
  { key: "color", label: "Text colour", group: "Fill", tokenKinds: ["color"] },
  { key: "background-color", label: "Background", group: "Fill", tokenKinds: ["color"] },
  { key: "border-radius", label: "Radius", group: "Fill", tokenKinds: ["radius"] },
];

const camel = (k: string) => k.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());

/** Form values from the bridge's computed styles (camelCase keys). */
export function fieldValues(computed: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of STYLE_FIELDS) out[f.key] = computed[camel(f.key)] ?? "";
  return out;
}

/** Changed fields only; an emptied field removes the property. */
export function styleChanges(before: Record<string, string>, after: Record<string, string>): StyleChange[] {
  return STYLE_FIELDS.filter((f) => (after[f.key] ?? "") !== (before[f.key] ?? "")).map((f) => ({
    property: f.key,
    value: (after[f.key] ?? "").trim(),
  }));
}

/** Mirrors `codemod::check_value`: values the Rust side will accept. */
export function safeValue(value: string): boolean {
  const v = value.toLowerCase();
  return value.length <= 200 && !/["'`;{}<>\\\n\r]/.test(value) && !v.includes("url(") && !v.includes("expression(") && !v.includes("@import");
}

/** Token suggestions for a field: `var(--name)` strings. */
export function tokenOptions(field: StyleField, tokens: DesignToken[]): string[] {
  if (!field.tokenKinds) return [];
  return tokens.filter((t) => field.tokenKinds!.includes(t.kind)).map((t) => `var(${t.name})`);
}

// ---- Bridge messages (preview iframe → studio) ----

export type BridgeMessage =
  | { ns: "next-agent"; type: "ready"; url: string }
  | { ns: "next-agent"; type: "select"; location: RawLocation; rect: { x: number; y: number; w: number; h: number }; styles: Record<string, string>; text: string }
  | { ns: "next-agent"; type: "styled"; styles: Record<string, string> }
  | { ns: "next-agent"; type: "nosource"; tag: string }
  | { ns: "next-agent"; type: "deselected" };

const ORIGINS = new Set(["attr", "svelte", "svelte4", "react", "vue"]);

/**
 * Accept a message only from the preview frame's window and the proxy's
 * origin, with the shape the bridge sends. Everything else is ignored.
 */
export function readBridgeMessage(
  event: { origin: string; source: unknown; data: unknown },
  expectedOrigin: string,
  frameWindow: unknown,
): BridgeMessage | null {
  if (!expectedOrigin || event.origin !== expectedOrigin || !frameWindow || event.source !== frameWindow) return null;
  const d = event.data as Record<string, unknown> | null;
  if (!d || typeof d !== "object" || d.ns !== "next-agent" || typeof d.type !== "string") return null;
  switch (d.type) {
    case "ready":
      return { ns: "next-agent", type: "ready", url: String(d.url ?? "") };
    case "select": {
      const l = d.location as Record<string, unknown> | undefined;
      if (!l || !ORIGINS.has(String(l.origin)) || typeof l.file !== "string") return null;
      const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 0 ? Math.floor(v) : 0);
      return {
        ns: "next-agent",
        type: "select",
        location: { origin: l.origin as RawLocation["origin"], file: l.file.slice(0, 1024), line: num(l.line), column: num(l.column), tag: String(l.tag ?? "").slice(0, 64) },
        rect: (d.rect as { x: number; y: number; w: number; h: number }) ?? { x: 0, y: 0, w: 0, h: 0 },
        styles: stringRecord(d.styles),
        text: String(d.text ?? "").slice(0, 80),
      };
    }
    case "styled":
      return { ns: "next-agent", type: "styled", styles: stringRecord(d.styles) };
    case "nosource":
      return { ns: "next-agent", type: "nosource", tag: String(d.tag ?? "").slice(0, 64) };
    case "deselected":
      return { ns: "next-agent", type: "deselected" };
    default:
      return null;
  }
}

function stringRecord(v: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) if (typeof x === "string") out[k] = x.slice(0, 200);
  return out;
}
