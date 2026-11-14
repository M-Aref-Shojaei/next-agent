#!/usr/bin/env node
// Coucou's code is MIT, but its name, the Mochi character, its icons, sounds
// and media are not (Coucou's LICENSE-ASSETS.md). We port the code only. This
// guard fails if any of those asset files lands in our tree, under the paths
// Coucou keeps them at (or copies of them):
//   *.xcassets/ (app and menu-bar icons), sounds/, docs/media/, design/
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const SKIP = new Set(["node_modules", "target", ".git", ".svelte-kit", "build", "dist", ".next-agent"]);
const RULES = [
  { test: (p) => /(^|\/)[^/]*\.xcassets(\/|$)/.test(p), why: "an asset catalog (*.xcassets)" },
  { test: (p) => /(^|\/)sounds\//.test(p), why: "a sounds/ folder" },
  { test: (p) => /^docs\/media\//.test(p), why: "docs/media/" },
  { test: (p) => /^design\//.test(p), why: "design/" },
];

const failures = [];

function walk(dir, rel = "") {
  for (const entry of readdirSync(dir)) {
    if (SKIP.has(entry)) continue;
    const path = rel ? `${rel}/${entry}` : entry;
    const full = join(dir, entry);
    for (const rule of RULES) {
      if (rule.test(path + (statSync(full).isDirectory() ? "/" : ""))) {
        failures.push(`${path}: ${rule.why} (Coucou assets are not ours to ship)`);
      }
    }
    if (statSync(full).isDirectory()) walk(full, path);
  }
}

walk(".");

if (failures.length) {
  console.error(`Coucou asset check failed:\n- ${[...new Set(failures)].join("\n- ")}`);
  process.exit(1);
}
console.log("Coucou asset check passed (no icons, sounds or media in the tree).");
