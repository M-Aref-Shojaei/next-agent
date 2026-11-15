// Tests the design-token checker (scripts/check-design-tokens.mjs, ADR-0012):
// it must catch raw colours and font stacks wherever style code hides them,
// and stay quiet on token use, selectors and non-style text.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  applyAllowlist,
  checkDeclaration,
  darkThemeParity,
  scanCss,
  scanFile,
  scanScript,
} from "../lib/design-tokens.mjs";

const whys = (list: { why: string }[]) => list.map((p) => p.why);

test("hex, colour functions and named colours are caught in declarations", () => {
  assert.match(checkDeclaration("color", "#fff")!, /raw colour #fff/);
  assert.match(checkDeclaration("background", "linear-gradient(#000, #12345678)")!, /raw colour/);
  assert.match(checkDeclaration("box-shadow", "0 1px 2px rgb(0 0 0 / 0.1)")!, /colour function/);
  assert.match(checkDeclaration("border", "1px solid hsl(0 0% 50%)")!, /colour function/);
  assert.match(checkDeclaration("background", "oklch(70% 0.1 200)")!, /colour function/);
  assert.match(checkDeclaration("color", "white")!, /named colour "white"/);
  assert.match(checkDeclaration("border", "1px solid Red")!, /named colour "red"/);
  assert.match(checkDeclaration("--tone", "tomato")!, /named colour/);
});

test("tokens, keywords and non-colour properties pass", () => {
  for (const [prop, value] of [
    ["color", "var(--color-text)"],
    ["background", "transparent"],
    ["border-color", "currentColor"],
    ["border", "1px solid var(--color-danger-red, x)"],
    ["fill", "none"],
    ["font-weight", "bold"],
    ["text-align", "left"],
    ["--accent", "var(--color-accent)"],
    ["--label", "orange juice"],
    ["font-family", "var(--font-mono)"],
    ["font-family", "inherit"],
  ]) {
    assert.equal(checkDeclaration(prop, value), null, `${prop}: ${value}`);
  }
});

test("font-family stacks are caught, camelCase too", () => {
  assert.match(checkDeclaration("font-family", "ui-monospace, monospace")!, /font-family/);
  assert.match(checkDeclaration("fontFamily", '"Inter"')!, /font-family/);
});

test("CSS: ids and pseudo-class selectors are not declarations; comments are ignored", () => {
  const css = `
    #fade, a:hover { color: var(--color-text); }
    /* color: #fff; */
    @media (prefers-color-scheme: dark) { .x { border: 1px solid var(--color-border); } }
    .y { background: #abc; }
  `;
  const found = scanCss(css);
  assert.equal(found.length, 1);
  assert.equal(found[0].line, 5);
});

test("Svelte: style blocks, style attributes, style: directives and colour attributes", () => {
  const svelte = [
    "<script>const c = { color: '#f00' };</script>", // 1
    '<div style="color: red; left: {x}px"></div>', // 2
    '<span style:background="#000"></span>', // 3
    '<svg><path fill="#123456" stroke="currentColor" /></svg>', // 4
    '<p>Issue #123 is fixed; color: brown text in markup is prose.</p>', // 5
    "<style>", // 6
    "  .a { color: var(--color-text); }", // 7
    "  .b { font-family: Menlo, monospace; }", // 8
    "</style>", // 9
  ].join("\n");
  const found = scanFile("x.svelte", svelte);
  assert.deepEqual(found.map((p) => p.line), [1, 2, 3, 4, 8]);
});

test("TS: style objects and style strings are scanned; other strings are not", () => {
  const ts = `
    const sticky = { backgroundColor: "#ffec99", label: "#1 idea" };
    const css = "color: white; padding: 4px";
    const url = "https://example.com/#fff"; // a comment with color: #fff
    const tag = "#abc";
  `;
  assert.deepEqual(whys(scanScript(ts)), ['raw colour #ffec99 in background-color', 'named colour "white" in color']);
});

test("the allowlist caps counts, notes progress and rejects stale entries", () => {
  const results = new Map([
    ["a.svelte", [{ line: 1, why: "x" }, { line: 2, why: "y" }]],
    ["b.svelte", [{ line: 1, why: "x" }]],
    ["c.svelte", []],
    ["d.svelte", [{ line: 3, why: "z" }]],
  ]);
  const allow = [
    { path: "a.svelte", max: 1 },
    { path: "b.svelte", max: 2 },
    { path: "c.svelte", max: 1 },
    { path: "gone.svelte", max: 1 },
  ];
  const { failures, notes } = applyAllowlist(results, allow);
  assert.ok(failures.some((f) => f.startsWith("a.svelte: 2 raw values")), failures.join("\n"));
  assert.ok(failures.some((f) => f.startsWith("c.svelte: allowlisted but now clean")));
  assert.ok(failures.some((f) => f.startsWith("gone.svelte: allowlisted but not found")));
  assert.ok(failures.some((f) => f.startsWith("d.svelte:3: z")));
  assert.equal(failures.length, 4);
  assert.equal(notes.length, 1);
  assert.match(notes[0], /^b\.svelte: down to 1/);
});

test("the real tokens.css keeps its two dark blocks in sync", () => {
  const css = readFileSync("apps/desktop/src/lib/ds/tokens.css", "utf8");
  assert.deepEqual(darkThemeParity(css), []);
  const broken = css.replace(/(\n\[data-theme="dark"\][\s\S]*?--color-bg: )#14161a/, "$1#000000");
  assert.equal(darkThemeParity(broken).length, 2);
});

test("the committed allowlist has no pending entry and caps every file", () => {
  const { entries } = JSON.parse(readFileSync("scripts/design-tokens-allowlist.json", "utf8"));
  assert.ok(!entries.some((e: { pending?: boolean }) => e.pending));
  for (const e of entries) assert.ok(Number.isInteger(e.max), e.path);
});

test("template interpolations and CONSTANT references are code, not colour words", () => {
  assert.equal(checkDeclaration("background", "${MAIL_BLUE}"), null);
  assert.deepEqual(scanScript('const a = { title: "GitHub", color: GITHUB_RED };'), []);
  assert.deepEqual(scanScript("el.setAttribute('style', `width:1px;background:${MAIL_BLUE}`);"), []);
  assert.equal(scanScript('el.style.cssText = "color: red";').length, 1);
});
