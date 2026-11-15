#!/usr/bin/env node
// Guards Rust sources while no compiler is available: balanced delimiters,
// no orphaned functions, declared modules present.
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { scan, orphanFunctions } from "./lib/rust-structure.mjs";

const SKIP = new Set(["node_modules", "target", ".git", ".next-agent"]);
const failures = [];

function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir)) {
    if (SKIP.has(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (entry.endsWith(".rs")) out.push(full);
  }
  return out;
}

function checkModules(file, source) {
  const dir = dirname(file);
  const lines = source.split("\n");
  let depth = 0;
  for (const text of lines) {
    const trimmed = text.trim();
    const mod = /^(pub(\([^)]*\))?\s+)?mod\s+([a-z0-9_]+)\s*;/.exec(trimmed);
    if (mod && depth === 0) {
      const name = mod[3];
      const flat = join(dir, `${name}.rs`);
      const nested = join(dir, name, "mod.rs");
      if (!existsSync(flat) && !existsSync(nested)) {
        failures.push(`${file}: mod ${name}; has no ${name}.rs or ${name}/mod.rs`);
      }
    }
    for (const char of text.replace(/\/\/.*$/, "").replace(/"(?:[^"\\]|\\.)*"/g, '""')) {
      if (char === "{") depth += 1;
      else if (char === "}") depth -= 1;
    }
    if (depth < 0) depth = 0;
  }
}

// Coucou's vendored code (ADR-0011) keeps upstream's syntax (raw strings,
// `#[path]` modules) that this heuristic misreads; the compiler checks it.
const VENDORED = "apps/desktop/coucou/";
const files = ["crates", "apps", "services"]
  .flatMap((root) => walk(root))
  .filter((file) => !file.replaceAll("\\", "/").startsWith(VENDORED));
for (const file of files) {
  const source = readFileSync(file, "utf8");
  for (const problem of scan(source)) failures.push(`${file}: ${problem}`);
  for (const line of orphanFunctions(source)) {
    failures.push(`${file}:${line}: fn declared outside an impl block`);
  }
  checkModules(file, source);
}

if (failures.length) {
  console.error(`Rust structure check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(`Rust structure check passed (${files.length} files).`);
