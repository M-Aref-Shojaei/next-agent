#!/usr/bin/env node
// The diagnostics log records how the app behaves, never what people work on
// (ADR-0013, docs/architecture/Logging.md). Fails when a log call uses a
// forbidden field name (prompt, content, title, path, token, key …) or one
// that is not on the allowlist in crates/diagnostics/src/fields.rs.
//
// Scans Rust `tracing` macros under crates/ and apps/ (not crates/diagnostics,
// whose tests plant forbidden names on purpose) and TypeScript `log.*` calls
// in the desktop and overlay frontends (not lib/log.ts, which defines `log`).
//
// scripts/log-fields-allowlist.json lists the calls that predate the log, one
// entry per file with its current count of non-allowlisted fields as "max":
// the count may only go down, and an entry must be removed once its file is
// clean. Wave 2 shrinks it to zero. Forbidden names are never allowlisted.
//
// Usage: node scripts/check-log-fields.mjs [--list]   (--list prints every finding)
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { applyAllowlist, checkCalls, readFieldLists, scanRust, scanTs } from "./lib/log-fields.mjs";

const FIELDS = "crates/diagnostics/src/fields.rs";
const ALLOWLIST = "scripts/log-fields-allowlist.json";
const RUST_ROOTS = ["crates", "apps"];
const TS_ROOTS = ["apps/desktop/src", "apps/desktop/coucou/src"];
const SKIP_DIRS = new Set(["node_modules", "target", ".git", ".svelte-kit", "build", "gen"]);
const SKIP_FILES = new Set(["apps/desktop/src/lib/log.ts"]);
const SKIP_PREFIX = "crates/diagnostics/";

function walk(dir, ext, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (SKIP_DIRS.has(entry.name) || entry.name.startsWith(".")) continue;
    const full = `${dir}/${entry.name}`;
    if (entry.isDirectory()) walk(full, ext, out);
    else if (ext.test(entry.name) && !entry.name.endsWith(".d.ts")) out.push(full);
  }
  return out;
}

const lists = readFieldLists(readFileSync(FIELDS, "utf8"));
const rust = RUST_ROOTS.flatMap((root) => walk(root, /\.rs$/)).filter((f) => !f.startsWith(SKIP_PREFIX));
const ts = TS_ROOTS.flatMap((root) => walk(root, /\.(ts|svelte)$/)).filter((f) => !SKIP_FILES.has(f));

const results = new Map();
let calls = 0;
for (const [files, scan] of [[rust, scanRust], [ts, scanTs]]) {
  for (const file of files.sort()) {
    const found = scan(readFileSync(file, "utf8"));
    calls += found.length;
    const problems = checkCalls(found, lists);
    if (problems.length) results.set(file, problems);
  }
}

if (process.argv.includes("--list")) {
  for (const [file, problems] of results) for (const p of problems) console.log(`${file}:${p.line}: ${p.why}`);
}

const allow = JSON.parse(readFileSync(ALLOWLIST, "utf8")).entries;
// Allowlisted files that are clean are not in `results`; give them an empty list.
for (const e of allow) if (!results.has(e.path) && existsSync(e.path)) results.set(e.path, []);
const { failures, notes } = applyAllowlist(results, allow);

for (const note of notes) console.log(`note: ${note}`);
if (failures.length) {
  console.error(`Log field check failed (docs/architecture/Logging.md):\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
const left = allow.reduce((n, e) => n + e.max, 0);
console.log(
  `Log field check passed (${calls} log calls in ${rust.length + ts.length} files; ${allow.length} pending files, ${left} fields left to fix).`,
);
