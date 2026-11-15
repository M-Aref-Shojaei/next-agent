#!/usr/bin/env node
// Drop allowlist entries the design-token check reports as clean.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const file = "scripts/design-tokens-allowlist.json";
let out = "";
try {
  out = execFileSync("node", ["scripts/check-design-tokens.mjs"], { encoding: "utf8", stdio: "pipe" });
} catch (e) {
  out = `${e.stdout ?? ""}${e.stderr ?? ""}`;
}
const clean = new Set(
  [...out.matchAll(/^- (.+): allowlisted but now clean/gm)].map((m) => m[1]),
);
const list = JSON.parse(readFileSync(file, "utf8"));
const before = list.entries.length;
list.entries = list.entries.filter((e) => !clean.has(e.path));
writeFileSync(file, `${JSON.stringify(list, null, 2)}\n`);
console.log(`Removed ${before - list.entries.length} clean entries; ${list.entries.length} left.`);
