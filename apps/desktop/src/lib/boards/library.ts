// Wireframe library authored for Next Agent (original shapes, no third-party
// library content). Each item is a group of skeletons laid out from (0, 0).
import type { Skeleton } from "./skeleton.ts";

export interface LibraryItem {
  id: string;
  name: string;
  elements: Skeleton[];
}

const INK = "#1e1e1e";
const MUTED = "#868e96";
const FILL = "#f1f3f5";
const LINE = { strokeColor: INK, roughness: 0, strokeWidth: 2 } as const;

function box(x: number, y: number, width: number, height: number, extra: Partial<Skeleton> = {}): Skeleton {
  return { type: "rectangle", x, y, width, height, ...LINE, roundness: { type: 3 }, ...extra };
}
function text(x: number, y: number, value: string, fontSize = 16, strokeColor = INK): Skeleton {
  return { type: "text", x, y, text: value, fontSize, strokeColor };
}

export const WIREFRAME_LIBRARY: readonly LibraryItem[] = [
  {
    id: "button",
    name: "Button",
    elements: [
      box(0, 0, 140, 44, { backgroundColor: "#4c6ef5", strokeColor: "#4c6ef5", fillStyle: "solid", label: { text: "Button", fontSize: 18, strokeColor: "#ffffff" } }),
    ],
  },
  {
    id: "button-secondary",
    name: "Button (secondary)",
    elements: [box(0, 0, 140, 44, { label: { text: "Cancel", fontSize: 18 } })],
  },
  {
    id: "input",
    name: "Text input",
    elements: [
      text(0, 0, "Label", 14, MUTED),
      box(0, 24, 280, 44, { backgroundColor: "#ffffff", fillStyle: "solid", label: { text: "Placeholder", fontSize: 16, textAlign: "left", strokeColor: MUTED } }),
    ],
  },
  {
    id: "checkbox",
    name: "Checkbox",
    elements: [box(0, 0, 24, 24, { roundness: { type: 3 } }), text(36, 2, "Option", 18)],
  },
  {
    id: "toggle",
    name: "Toggle",
    elements: [
      box(0, 0, 56, 28, { roundness: { type: 3 }, backgroundColor: "#4c6ef5", strokeColor: "#4c6ef5", fillStyle: "solid" }),
      { type: "ellipse", x: 30, y: 3, width: 22, height: 22, ...LINE, strokeColor: "#ffffff", backgroundColor: "#ffffff", fillStyle: "solid" },
      text(70, 3, "Enabled", 18),
    ],
  },
  {
    id: "avatar",
    name: "Avatar",
    elements: [{ type: "ellipse", x: 0, y: 0, width: 56, height: 56, ...LINE, backgroundColor: FILL, fillStyle: "solid", label: { text: "AB", fontSize: 18 } }],
  },
  {
    id: "card",
    name: "Card",
    elements: [
      box(0, 0, 280, 260, { backgroundColor: "#ffffff", fillStyle: "solid" }),
      box(16, 16, 248, 120, { backgroundColor: FILL, fillStyle: "solid", strokeStyle: "dashed", label: { text: "Image", fontSize: 16, strokeColor: MUTED } }),
      text(16, 152, "Card title", 20),
      text(16, 184, "Short supporting text that\nexplains the card.", 14, MUTED),
      box(16, 216, 100, 28, { label: { text: "Action", fontSize: 14 } }),
    ],
  },
  {
    id: "list-item",
    name: "List item",
    elements: [
      box(0, 0, 360, 64, { backgroundColor: "#ffffff", fillStyle: "solid" }),
      { type: "ellipse", x: 12, y: 12, width: 40, height: 40, ...LINE, backgroundColor: FILL, fillStyle: "solid" },
      text(68, 12, "Item title", 18),
      text(68, 36, "Secondary text", 14, MUTED),
      text(332, 20, "›", 24, MUTED),
    ],
  },
  {
    id: "nav-bar",
    name: "Navigation bar",
    elements: [
      box(0, 0, 800, 56, { backgroundColor: "#ffffff", fillStyle: "solid", roundness: null }),
      text(20, 16, "Logo", 22),
      text(460, 18, "Home", 16),
      text(540, 18, "Features", 16),
      text(640, 18, "Pricing", 16),
      box(716, 10, 70, 36, { backgroundColor: "#4c6ef5", strokeColor: "#4c6ef5", fillStyle: "solid", label: { text: "Sign in", fontSize: 14, strokeColor: "#ffffff" } }),
    ],
  },
  {
    id: "tab-bar",
    name: "Tab bar (mobile)",
    elements: [
      box(0, 0, 360, 64, { backgroundColor: "#ffffff", fillStyle: "solid", roundness: null }),
      { type: "ellipse", x: 38, y: 12, width: 24, height: 24, ...LINE },
      text(36, 40, "Home", 12),
      { type: "ellipse", x: 168, y: 12, width: 24, height: 24, ...LINE },
      text(160, 40, "Search", 12),
      { type: "ellipse", x: 298, y: 12, width: 24, height: 24, ...LINE },
      text(290, 40, "Profile", 12),
    ],
  },
  {
    id: "modal",
    name: "Modal dialog",
    elements: [
      box(0, 0, 440, 280, { backgroundColor: "#ffffff", fillStyle: "solid" }),
      text(24, 24, "Dialog title", 22),
      text(24, 68, "Explain what will happen and what\nthe person needs to decide.", 16, MUTED),
      box(24, 220, 120, 40, { label: { text: "Cancel", fontSize: 16 } }),
      box(296, 220, 120, 40, { backgroundColor: "#4c6ef5", strokeColor: "#4c6ef5", fillStyle: "solid", label: { text: "Confirm", fontSize: 16, strokeColor: "#ffffff" } }),
      text(408, 18, "×", 28, MUTED),
    ],
  },
  {
    id: "phone-frame",
    name: "Phone frame",
    elements: [
      box(0, 0, 390, 780, { backgroundColor: "#ffffff", fillStyle: "solid", roundness: { type: 3 }, strokeWidth: 3 }),
      box(135, 12, 120, 24, { roundness: { type: 3 }, backgroundColor: INK, fillStyle: "solid" }),
      text(28, 14, "9:41", 14),
      box(145, 758, 100, 6, { roundness: { type: 3 }, backgroundColor: INK, fillStyle: "solid" }),
    ],
  },
  {
    id: "desktop-frame",
    name: "Desktop frame",
    elements: [
      box(0, 0, 1040, 680, { backgroundColor: "#ffffff", fillStyle: "solid", roundness: { type: 3 } }),
      box(0, 0, 1040, 40, { backgroundColor: FILL, fillStyle: "solid", roundness: null }),
      { type: "ellipse", x: 16, y: 13, width: 14, height: 14, ...LINE, strokeWidth: 1 },
      { type: "ellipse", x: 38, y: 13, width: 14, height: 14, ...LINE, strokeWidth: 1 },
      { type: "ellipse", x: 60, y: 13, width: 14, height: 14, ...LINE, strokeWidth: 1 },
      box(220, 8, 600, 24, { backgroundColor: "#ffffff", fillStyle: "solid", label: { text: "app.example.com", fontSize: 14, strokeColor: MUTED } }),
    ],
  },
  {
    id: "image-placeholder",
    name: "Image placeholder",
    elements: [
      box(0, 0, 320, 200, { backgroundColor: FILL, fillStyle: "solid", strokeStyle: "dashed", roundness: null }),
      { type: "line", x: 0, y: 0, points: [[0, 0], [320, 200]], ...LINE, strokeColor: MUTED, strokeWidth: 1 },
      { type: "line", x: 320, y: 0, points: [[0, 0], [-320, 200]], ...LINE, strokeColor: MUTED, strokeWidth: 1 },
    ],
  },
];
