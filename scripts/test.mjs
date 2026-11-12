#!/usr/bin/env node
// Runs every node:test suite in the workspace. Uses Node's built-in test
// runner and native TypeScript support, so no dependencies are required.
import { spawnSync } from "node:child_process";
import { readdirSync, statSync, existsSync } from "node:fs";
import { join } from "node:path";

const roots = ["packages", "services", "apps", "scripts"];
const patterns = [/\.test\.[cm]?[jt]s$/, /\.spec\.[cm]?[jt]s$/];
const files = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry.startsWith(".")) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full);
    else if (patterns.some((p) => p.test(entry))) files.push(full);
  }
}

for (const root of roots) if (existsSync(root)) walk(root);

if (files.length === 0) {
  console.log("No TypeScript tests yet.");
  process.exit(0);
}

console.log(`Running ${files.length} test file(s) with node:test`);
const result = spawnSync(process.execPath, ["--test", "--enable-source-maps", ...files], {
  stdio: "inherit",
});
process.exit(result.status ?? 1);
