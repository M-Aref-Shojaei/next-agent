#!/usr/bin/env node
// One design system (ADR-0012): style code under apps/desktop/src and the
// vendored Coucou UI (apps/desktop/coucou/src, except its settings window, which
// is moved into Next Agent Settings and is still "pending" in the allowlist) uses
// the tokens in apps/desktop/src/lib/ds/tokens.css, never raw colours or font
// stacks. Fails on any raw value outside tokens.css that the allowlist does not
// cover. scripts/design-tokens-allowlist.json lists today's offenders, one entry
// per file with its current count as "max": the count may only go down, and an
// entry must be removed once its file is clean. Later tasks shrink it to zero.
//
// Usage: node scripts/check-design-tokens.mjs [--list]   (--list prints every finding)
import { readFileSync, readdirSync } from "node:fs";
import { applyAllowlist, darkThemeParity, scanFile } from "./lib/design-tokens.mjs";

const ROOT = "apps/desktop/src";
const COUCOU = "apps/desktop/coucou/src";
const COUCOU_SETTINGS = `${COUCOU}/settings/`; // pending: not scanned until it moves into Settings
const TOKENS = `${ROOT}/lib/ds/tokens.css`;
const ALLOWLIST = "scripts/design-tokens-allowlist.json";
const EXT = /\.(svelte|css|[cm]?ts)$/;

const files = [ROOT, COUCOU]
  .flatMap((root) => readdirSync(root, { recursive: true, withFileTypes: true }))
  .filter((e) => e.isFile() && EXT.test(e.name) && !e.name.endsWith(".d.ts"))
  .map((e) => `${e.parentPath}/${e.name}`.split("\\").join("/"))
  .filter((f) => !f.startsWith(COUCOU_SETTINGS))
  .sort();

const results = new Map();
for (const file of files) {
  if (file === TOKENS) continue;
  results.set(file, scanFile(file, readFileSync(file, "utf8")));
}

if (process.argv.includes("--list")) {
  for (const [file, problems] of results) for (const p of problems) console.log(`${file}:${p.line}: ${p.why}`);
}

const allow = JSON.parse(readFileSync(ALLOWLIST, "utf8")).entries;
const { failures, notes } = applyAllowlist(results, allow);
failures.push(...darkThemeParity(readFileSync(TOKENS, "utf8")));

for (const note of notes) console.log(`note: ${note}`);
if (failures.length) {
  console.error(`Design token check failed (docs/design/Design System.md):\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
const listed = allow.filter((e) => !e.pending);
const left = listed.reduce((n, e) => n + e.max, 0);
const pending = allow.filter((e) => e.pending).map((e) => e.path).join(", ");
console.log(`Design token check passed (${listed.length} allowlisted files, ${left} raw values left to move to tokens; pending, not scanned: ${pending || "none"}).`);
