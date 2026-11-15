// Browser-only stand-in for the Design Studio commands (vite dev, screenshots).
// The real flows (critique, worktree tasks, preview proxy) live in Rust.
import type { Api } from "./api.ts";
import { mockPageCss } from "./design-preview-data.ts";
import type { DesignDirection, DesignFiles, DesignToken, FrameKind } from "./design.ts";

type DesignApi = Pick<
  Api,
  | "designSystemGet"
  | "designSystemGenerate"
  | "designSystemSave"
  | "designGenerate"
  | "designHistory"
  | "designChoose"
  | "designIterate"
  | "designPreviewStart"
  | "designPreviewStop"
  | "designPreviewLog"
  | "designSource"
  | "designApplyStyle"
  | "designConvert"
  | "designImplement"
>;

const TOKENS: DesignToken[] = [
  { name: "--bg", value: "#fafafa", dark: "#14161a", kind: "color", derived: false },
  { name: "--panel", value: "#ffffff", dark: "#1b1e23", kind: "color", derived: false },
  { name: "--text", value: "#1b1d21", dark: "#e6e8ec", kind: "color", derived: false },
  { name: "--muted", value: "#666b75", dark: "#9097a3", kind: "color", derived: false },
  { name: "--accent", value: "#3b5bdb", dark: "#7c93f5", kind: "color", derived: false },
  { name: "--border", value: "#dcdee3", dark: "#2c3038", kind: "color", derived: false },
  { name: "--radius-1", value: "6px", kind: "radius", derived: true },
  { name: "--space-1", value: "4px", kind: "spacing", derived: true },
  { name: "--space-2", value: "8px", kind: "spacing", derived: true },
];
const css = (tokens: DesignToken[]) =>
  `:root {\n${tokens.map((t) => `  ${t.name}: ${t.value};`).join("\n")}\n}\n@media (prefers-color-scheme: dark) {\n  :root {\n${tokens
    .filter((t) => t.dark)
    .map((t) => `    ${t.name}: ${t.dark};`)
    .join("\n")}\n  }\n}\n`;

let files: DesignFiles = {
  designPath: "design/DESIGN.md",
  tokensPath: "design/tokens.css",
  designMd: "# Demo design system\n\n## 1. Visual Theme & Atmosphere\n\n- Restrained, functional interface.\n",
  tokensCss: css(TOKENS),
  saved: false,
  tokens: TOKENS,
  sources: ["src/app.css"],
};
const directions: DesignDirection[] = [];
let n = 0;

const POSTURES = [
  ["modern-minimal", "Modern minimal", "8px", "16px"],
  ["tech-utility", "Tech utility", "4px", "8px"],
  ["human-approachable", "Human approachable", "16px", "20px"],
  ["editorial-monocle", "Editorial", "0", "24px"],
  ["brutalist-experimental", "Bold experimental", "0", "12px"],
] as const;

function html(title: string, radius: string, gap: string): string {
  const cards = ["Get started", "Recent work", "Shortcuts"]
    .map((c) => `<section class="card"><h2>${c}</h2><p>Short, specific description.</p><button>Open</button></section>`)
    .join("");
  return `<!doctype html><html><head><style>${files.tokensCss}
${mockPageCss(radius, gap)}</style></head>
<body><header><h1>${title.replace(/</g, "&lt;")}</h1><button class="primary">New</button></header><main>${cards}</main></body></html>`;
}

const srcdoc = (doc: string) =>
  doc.replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:">`);

function direction(prompt: string, frame: FrameKind, batchId: string, i: number): DesignDirection {
  const [identifier, title, radius, gap] = POSTURES[i % POSTURES.length]!;
  const doc = html(prompt.split("\n")[0] ?? "Untitled", radius, gap);
  return {
    id: `mock_dir_${++n}`,
    batchId,
    parentId: null,
    prompt,
    frame,
    identifier,
    title,
    rationale: "Offline browser mock (run the desktop app for real generation).",
    html: doc,
    srcdoc: srcdoc(doc),
    critique: { scores: [["philosophy", 4], ["hierarchy", 4], ["execution", 4], ["specificity", 3], ["restraint", 4]], notes: "Mock review.", revised: false },
    lint: [],
    note: "",
    reply: "",
    chosen: false,
    createdAt: new Date().toISOString(),
  };
}

const desktopOnly = "Edit mode needs the desktop app (the preview proxy runs in Rust).";

export const designMockApi: DesignApi = {
  async designSystemGet() {
    return files;
  },
  async designSystemGenerate(_ws, overwrite) {
    if (files.saved && !overwrite) throw "exists: DESIGN.md or tokens.css is already saved; confirm to replace them";
    files = { ...files, saved: true };
    return files;
  },
  async designSystemSave(_ws, designMd, tokensCss) {
    files = { ...files, designMd, tokensCss, saved: true };
    return files;
  },
  async designGenerate(_ws, prompt, frame, count, _board, critique, onProgress) {
    const total = Math.min(5, Math.max(3, count));
    onProgress({ phase: "generating", count: total });
    await new Promise((r) => setTimeout(r, 400));
    const batchId = `mock_batch_${++n}`;
    const made = Array.from({ length: total }, (_, i) => direction(prompt, frame, batchId, i));
    if (critique) {
      for (const [i, d] of made.entries()) {
        onProgress({ phase: "critiquing", index: i, total, title: d.title });
        await new Promise((r) => setTimeout(r, 120));
      }
    }
    onProgress({ phase: "done" });
    directions.unshift(...made);
    return { batchId, directions: made, model: { label: "offline mock", mock: true, reason: "browser preview" }, notices: [] };
  },
  async designHistory() {
    return directions;
  },
  async designChoose(id) {
    const d = directions.find((x) => x.id === id);
    for (const x of directions) if (x.batchId === d?.batchId) x.chosen = x.id === id;
  },
  async designIterate(id, message) {
    const base = directions.find((x) => x.id === id);
    if (!base) throw `unknown direction ${id}`;
    const doc = base.html.replace("</body>", `<p style="padding:8px 16px;opacity:.7">Revised: ${message.replace(/</g, "&lt;")}</p></body>`);
    const next: DesignDirection = { ...base, id: `mock_dir_${++n}`, parentId: base.id, html: doc, srcdoc: srcdoc(doc), note: message, reply: `Applied “${message}” (mock).`, critique: null, chosen: true };
    for (const x of directions) if (x.batchId === base.batchId) x.chosen = false;
    directions.unshift(next);
    return next;
  },
  async designPreviewStart() {
    throw desktopOnly;
  },
  async designPreviewStop() {},
  async designPreviewLog() {
    return [];
  },
  async designSource() {
    throw desktopOnly;
  },
  async designApplyStyle() {
    throw desktopOnly;
  },
  async designConvert() {
    throw "Conversion runs as a task in the desktop app.";
  },
  async designImplement() {
    throw "Implement runs as a task in the desktop app.";
  },
};
