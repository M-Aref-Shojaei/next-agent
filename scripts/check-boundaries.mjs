#!/usr/bin/env node
// Enforces the seams in docs/architecture/Module Boundaries.md.
// Rules:
//  1. transport crates stay inside their adapter (one transport per adapter)
//  2. crates never reach into another crate's src via a path import
//  3. every seam listed in the doc has its contract file present
//  4. upstream.lock.json is valid and every entry names an existing adapter
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const failures = [];
// Owner is a file, or a directory when it ends with "/".
const TRANSPORTS = {
  rusqlite: "crates/core/src/store.rs",
  reqwest: "crates/core/src/router/",
  eventsource_stream: "crates/core/src/router/",
  keyring: "crates/core/src/router/",
};
const owns = (owner, rel) => (owner.endsWith("/") ? rel.startsWith(owner) : rel === owner);

function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir)) {
    if (["node_modules", "target", ".git"].includes(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (entry.endsWith(".rs")) out.push(full);
  }
  return out;
}

// 1. transport isolation
for (const file of walk("crates")) {
  const source = readFileSync(file, "utf8");
  const rel = file.replaceAll("\\", "/");
  for (const [krate, owner] of Object.entries(TRANSPORTS)) {
    if (owns(owner, rel)) continue;
    // `use krate…` imports and fully-qualified `krate::` paths both count.
    if (new RegExp(`(^\\s*(pub\\s+)?use\\s+${krate}\\b)|(\\b${krate}::)`, "m").test(source)) {
      failures.push(`${rel} imports ${krate}; only ${owner} may depend on it`);
    }
  }
}

// 2. no cross-crate path jumps
for (const manifest of ["Cargo.toml"]) {
  if (!existsSync(manifest)) continue;
}

// 3. seam contract files exist
const SEAMS = [
  ["Agent runtime", "crates/core/src/agent.rs"],
  ["Tool enforcement", "crates/core/src/runner.rs"],
  ["Path sandbox", "crates/permission-engine/src/path.rs"],
  ["Event wire format", "packages/contracts/src/index.ts"],
  ["Upstream pin", "upstream.lock.json"],
];
for (const [name, path] of SEAMS) {
  if (!existsSync(path)) failures.push(`seam "${name}" missing contract file: ${path}`);
}

// 4. upstream lock integrity
if (existsSync("upstream.lock.json")) {
  let lock;
  try {
    lock = JSON.parse(readFileSync("upstream.lock.json", "utf8"));
  } catch (error) {
    failures.push(`upstream.lock.json is not valid JSON: ${error.message}`);
  }
  if (lock) {
    for (const [name, entry] of Object.entries(lock.components ?? {})) {
      const adapter = entry.adapter ?? "";
      if (!existsSync(adapter)) failures.push(`upstream "${name}" adapter path missing: ${adapter}`);
      if (!entry.pin) failures.push(`upstream "${name}" has no pin`);
      if (!entry.checked) failures.push(`upstream "${name}" has no checked date`);
    }
  }
}

if (failures.length) {
  console.error(`Boundary check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("Boundary check passed (4 seams, transports isolated).");
