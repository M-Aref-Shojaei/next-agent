import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  APP_SANDBOX,
  DIRECTION_SANDBOX,
  averageScore,
  batchHeads,
  fieldValues,
  fitScale,
  lineage,
  progressLabel,
  readBridgeMessage,
  safeValue,
  styleChanges,
  tokenOptions,
  STYLE_FIELDS,
  type DesignDirection,
} from "../src/lib/design.ts";

const here = new URL(".", import.meta.url);
const read = (p: string) => readFileSync(new URL(p, here), "utf8");

test("sandbox constants match the Rust side and the page uses them", () => {
  const rust = read("../../../crates/design/src/sandbox.rs");
  assert.match(rust, new RegExp(`pub const DIRECTION_SANDBOX: &str = "${DIRECTION_SANDBOX}";`));
  assert.match(rust, new RegExp(`pub const APP_SANDBOX: &str = "${APP_SANDBOX}";`));
  const page = read("../src/routes/(app)/design/+page.svelte") + read("../src/lib/components/DesignFrame.svelte");
  // Every iframe takes its sandbox from these constants (or the preview DTO).
  const iframes = page.match(/<iframe[\s\S]*?>/g) ?? [];
  assert.ok(iframes.length > 0);
  for (const tag of iframes) {
    assert.match(tag, /sandbox=\{/, `iframe without a bound sandbox: ${tag}`);
    assert.doesNotMatch(tag, /allow-top-navigation|allow-popups|allow-downloads/);
  }
  assert.doesNotMatch(APP_SANDBOX, /allow-top-navigation|allow-popups|allow-modals/);
});

test("CSP stays strict: frames only from self and the loopback proxy", () => {
  const conf = JSON.parse(read("../src-tauri/tauri.conf.json"));
  const csp: string = conf.app.security.csp;
  const frame = /frame-src ([^;]+)/.exec(csp)?.[1] ?? "";
  assert.deepEqual(frame.trim().split(/\s+/).sort(), ["'self'", "http://127.0.0.1:*"].sort());
  assert.match(csp, /default-src 'self'/);
  assert.doesNotMatch(csp, /script-src[^;]*unsafe/);
  assert.doesNotMatch(csp, /https?:\/\/\*/);
});

test("bridge messages are accepted only from the frame and origin", () => {
  const frame = {};
  const ok = { origin: "http://127.0.0.1:4000", source: frame, data: { ns: "next-agent", type: "select", location: { origin: "svelte", file: "src/a.svelte", line: 3, column: -1, tag: "H1" }, styles: { color: "red", bad: 1 }, text: "x" } };
  const msg = readBridgeMessage(ok, "http://127.0.0.1:4000", frame);
  assert.equal(msg?.type, "select");
  if (msg?.type === "select") {
    assert.equal(msg.location.column, 0, "negative numbers clamp");
    assert.deepEqual(msg.styles, { color: "red" });
  }
  assert.equal(readBridgeMessage({ ...ok, origin: "http://evil" }, "http://127.0.0.1:4000", frame), null);
  assert.equal(readBridgeMessage({ ...ok, source: {} }, "http://127.0.0.1:4000", frame), null);
  assert.equal(readBridgeMessage(ok, "", frame), null);
  assert.equal(readBridgeMessage({ ...ok, data: { ns: "x", type: "ready" } }, "http://127.0.0.1:4000", frame), null);
  assert.equal(readBridgeMessage({ ...ok, data: { ns: "next-agent", type: "select", location: { origin: "eval", file: "a" } } }, "http://127.0.0.1:4000", frame), null);
  assert.equal(readBridgeMessage({ ...ok, data: { ns: "next-agent", type: "run" } }, "http://127.0.0.1:4000", frame), null);
});

test("style inspector: values, changes, safety, tokens", () => {
  const before = fieldValues({ display: "block", color: "rgb(0, 0, 0)", paddingTop: "1px" });
  assert.equal(before["display"], "block");
  assert.equal(before["padding"], "");
  const after = { ...before, display: "flex", color: "", gap: "var(--space-2)" };
  assert.deepEqual(styleChanges(before, after), [
    { property: "display", value: "flex" },
    { property: "gap", value: "var(--space-2)" },
    { property: "color", value: "" },
  ]);
  for (const bad of ['red" onclick="x', "a;b", "url(x)", "</style>", "a\nb", "a\tb"]) assert.equal(safeValue(bad), false, bad);
  assert.equal(safeValue("var(--accent)"), true);
  const gap = STYLE_FIELDS.find((f) => f.key === "gap")!;
  assert.deepEqual(tokenOptions(gap, [{ name: "--space-1", value: "4px", kind: "spacing", derived: true }, { name: "--bg", value: "#fff", kind: "color", derived: false }]), ["var(--space-1)"]);
});

test("frames, progress and critique helpers", () => {
  assert.equal(fitScale("mobile", 195), 0.5);
  assert.equal(fitScale("web", 5000), 1);
  assert.equal(fitScale("web", 0), 0.1);
  assert.equal(progressLabel({ phase: "critiquing", index: 0, total: 3, title: "Calm" }), "Self-critique 1/3: Calm");
  assert.equal(averageScore({ scores: [["a", 4], ["b", 3]], notes: "", revised: false }), 3.5);
  assert.equal(averageScore(null), null);
});

test("lineage and batch heads follow refinements", () => {
  const d = (id: string, parentId: string | null, batchId = "b"): DesignDirection => ({ id, parentId, batchId, prompt: "", frame: "web", identifier: id, title: id, rationale: "", html: "", srcdoc: "", critique: null, lint: [], note: "", reply: "", chosen: false, createdAt: "" });
  const all = [d("a2", "a1"), d("a1", null), d("b1", null), d("a3", "a2"), d("z", null, "other")];
  assert.deepEqual(lineage(all, "a3").map((x) => x.id), ["a1", "a2", "a3"]);
  assert.deepEqual(batchHeads(all, "b").map((x) => x.id), ["a3", "b1"]);
});
