// Board templates, authored as skeleton data (see skeleton.ts).
import { sticky, STICKY_PRESETS, STICKY_SIZE } from "./stickies.ts";
import { WIREFRAME_LIBRARY } from "./library.ts";
import type { Skeleton } from "./skeleton.ts";

export interface BoardTemplate {
  id: string;
  name: string;
  description: string;
  elements: Skeleton[];
}

const INK = "#1e1e1e";
const base = { strokeColor: INK, roughness: 0, strokeWidth: 2 } as const;

function step(id: string, x: number, y: number, label: string): Skeleton {
  return { type: "rectangle", id, x, y, width: 180, height: 80, ...base, roundness: { type: 3 }, backgroundColor: "#e7f5ff", fillStyle: "solid", label: { text: label, fontSize: 18 } };
}
function link(from: string, to: string, label?: string): Skeleton {
  return { type: "arrow", x: 0, y: 0, ...base, start: { id: from }, end: { id: to }, ...(label ? { label: { text: label, fontSize: 14 } } : {}) };
}

const userFlow: Skeleton[] = [
  { type: "ellipse", id: "start", x: 0, y: 0, width: 140, height: 70, ...base, backgroundColor: "#d3f9d8", fillStyle: "solid", label: { text: "Start", fontSize: 18 } },
  step("landing", 220, -5, "Landing page"),
  step("signup", 480, -5, "Sign up form"),
  { type: "diamond", id: "valid", x: 760, y: -30, width: 200, height: 130, ...base, backgroundColor: "#fff3bf", fillStyle: "solid", label: { text: "Valid?", fontSize: 18 } },
  step("error", 740, 170, "Show errors"),
  step("home", 1040, -5, "Dashboard"),
  { type: "ellipse", id: "end", x: 1300, y: 0, width: 140, height: 70, ...base, backgroundColor: "#ffe3e3", fillStyle: "solid", label: { text: "Done", fontSize: 18 } },
  link("start", "landing"),
  link("landing", "signup"),
  link("signup", "valid"),
  link("valid", "home", "yes"),
  link("valid", "error", "no"),
  link("error", "signup"),
  link("home", "end"),
];

function wireframePage(): Skeleton[] {
  const get = (id: string) => WIREFRAME_LIBRARY.find((i) => i.id === id)?.elements ?? [];
  const shift = (items: Skeleton[], dx: number, dy: number, prefix: string): Skeleton[] =>
    items.map((i) => ({ ...i, x: i.x + dx, y: i.y + dy, id: i.id ? `${prefix}${i.id}` : undefined }));
  return [
    ...shift(get("desktop-frame"), 0, 0, "d-"),
    ...shift(get("nav-bar"), 120, 60, "n-"),
    { type: "rectangle", x: 120, y: 140, width: 800, height: 180, ...base, backgroundColor: "#f1f3f5", fillStyle: "solid", roundness: { type: 3 }, label: { text: "Hero: headline, subtitle, call to action", fontSize: 20 } },
    ...shift(get("card"), 120, 350, "c1-"),
    ...shift(get("card"), 380, 350, "c2-"),
    ...shift(get("card"), 640, 350, "c3-"),
  ];
}

function retrospective(): Skeleton[] {
  const columns = [
    { id: "good", title: "Went well", color: STICKY_PRESETS[3]!.color },
    { id: "bad", title: "To improve", color: STICKY_PRESETS[2]!.color },
    { id: "act", title: "Actions", color: STICKY_PRESETS[0]!.color },
  ];
  const colWidth = STICKY_SIZE * 2 + 60;
  const out: Skeleton[] = [];
  columns.forEach((col, c) => {
    const x = c * (colWidth + 40);
    const ids: string[] = [];
    for (let n = 0; n < 4; n += 1) {
      const id = `${col.id}-${n}`;
      ids.push(id);
      out.push(sticky(col.color, "Add a note", x + 20 + (n % 2) * (STICKY_SIZE + 20), 60 + Math.floor(n / 2) * (STICKY_SIZE + 20), id));
    }
    out.push({ type: "frame", id: `frame-${col.id}`, x, y: 0, width: colWidth, height: 60 + 2 * (STICKY_SIZE + 20) + 20, name: col.title, children: ids });
  });
  return out;
}

export const TEMPLATES: readonly BoardTemplate[] = [
  { id: "blank", name: "Blank", description: "An empty canvas.", elements: [] },
  { id: "user-flow", name: "User flow", description: "Start, steps, a decision and an error loop.", elements: userFlow },
  { id: "wireframe-page", name: "Wireframe page", description: "Desktop frame with nav bar, hero and three cards.", elements: wireframePage() },
  { id: "retrospective", name: "Retrospective", description: "Went well, to improve and actions with sticky notes.", elements: retrospective() },
];
