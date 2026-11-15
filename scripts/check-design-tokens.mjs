#!/usr/bin/env node
// One design system (ADR-0012): style code under apps/desktop/src uses the
// tokens in apps/desktop/src/lib/ds/tokens.css, never raw colours or font
// stacks. Fails on any raw value outside tokens.css that the allowlist does not
// cover. scripts/design-tokens-allowlist.json lists today's offenders, one entry
// per file with its current count as "max": the count may only go down, and an
// entry must be removed once its file is clean. Later tasks shrink it to zero.
//
// Usage: node scripts/check-design-tokens.mjs [--list]   (--list prints every finding)
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { applyAllowlist, darkThemeParity, scanFile } from "./lib/design-tokens.mjs";

const ROOT = "apps/desktop/src";
const TOKENS = `${ROOT}/lib/ds/tokens.css`;
const ALLOWLIST = "scripts/design-tokens-allowlist.json";
const EXT = /\.(svelte|css|[cm]?ts)$/;

function walk(dir, files = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, files);
    else if (EXT.test(entry) && !entry.endsWith(".d.ts")) files.push(relative(".", full).split("\\").join("/"));
  }
  return files;
}

const results = new Map();
for (const file of walk(ROOT)) {
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
const pending = allow.reduce((n, e) => n + (e.max ?? 0), 0);
console.log(`Design token check passed (${allow.length} allowlisted entries, ${pending} raw values left to move to tokens).`);
