#!/usr/bin/env node
// Runs every node:test suite in the workspace. Uses Node's built-in test
// runner and native TypeScript support, so no dependencies are required.
//
// A folder whose tests need a setup module of their own (Coucou's vendored
// suite, apps/desktop/coucou/tests/setup.mjs) runs as its own suite with it,
// exactly as upstream runs it (`node --import ./tests/setup.mjs --test`).
import { spawnSync } from "node:child_process";
import { readdirSync, statSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";

const roots = ["packages", "services", "apps", "scripts"];
const patterns = [/\.test\.[cm]?[jt]s$/, /\.spec\.[cm]?[jt]s$/];
/** Test folders that bring their own setup: folder → setup module. */
const SUITES = { [join("apps", "desktop", "coucou", "tests")]: "setup.mjs" };
const files = [];
const suites = new Map();

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry.startsWith(".")) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full);
    else if (!full.startsWith(join("apps", "desktop", "tests", "ui")) && patterns.some((p) => p.test(entry))) {
      const folder = dirname(full);
      if (SUITES[folder]) suites.set(folder, [...(suites.get(folder) ?? []), full]);
      else files.push(full);
    }
  }
}

for (const root of roots) if (existsSync(root)) walk(root);

if (files.length === 0 && suites.size === 0) {
  console.log("No TypeScript tests yet.");
  process.exit(0);
}

let status = 0;
if (files.length > 0) {
  console.log(`Running ${files.length} test file(s) with node:test`);
  const result = spawnSync(process.execPath, ["--test", "--enable-source-maps", ...files], {
    stdio: "inherit",
  });
  status ||= result.status ?? 1;
}
for (const [folder, suiteFiles] of suites) {
  console.log(`Running ${suiteFiles.length} test file(s) in ${folder} with ${SUITES[folder]}`);
  const result = spawnSync(
    process.execPath,
    ["--import", `./${join(folder, SUITES[folder])}`, "--test", "--enable-source-maps", ...suiteFiles],
    { stdio: "inherit" },
  );
  status ||= result.status ?? 1;
}
process.exit(status);
