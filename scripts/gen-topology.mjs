#!/usr/bin/env node
// Writes docs/architecture/Project Topology.md (a Mermaid diagram of how the
// repo's crates, packages and services connect). `--check` fails if stale.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { collect, renderNote } from "./lib/topology.mjs";

const target = "docs/architecture/Project Topology.md";
const note = renderNote(collect("."));
const current = existsSync(target) ? readFileSync(target, "utf8") : null;

if (process.argv.includes("--check")) {
  if (current !== note) {
    console.error(`${target} is out of date. Run: make topology`);
    process.exit(1);
  }
  console.log("Topology note is up to date.");
} else if (current === note) {
  console.log(`${target} unchanged.`);
} else {
  writeFileSync(target, note);
  console.log(`Wrote ${target}.`);
}
