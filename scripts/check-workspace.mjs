#!/usr/bin/env node
// Validates that the Cargo workspace, pnpm workspace and on-disk crates agree.
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

const failures = [];
const cargo = readFileSync("Cargo.toml", "utf8");
const membersBlock = /members\s*=\s*\[([\s\S]*?)\]/.exec(cargo)?.[1] ?? "";
const members = [...membersBlock.matchAll(/"([^"]+)"/g)].map((m) => m[1]);

for (const member of members) {
  if (!existsSync(join(member, "Cargo.toml"))) {
    failures.push(`Cargo member missing manifest: ${member}`);
  }
}
for (const dir of readdirSync("crates")) {
  const manifest = join("crates", dir, "Cargo.toml");
  if (existsSync(manifest) && !cargo.includes(`"crates/${dir}"`)) {
    failures.push(`crate not listed in workspace members: crates/${dir}`);
  }
}

if (existsSync("pnpm-workspace.yaml")) {
  const pnpm = readFileSync("pnpm-workspace.yaml", "utf8");
  for (const required of ["apps/desktop", "packages/*", "services/*"]) {
    if (!pnpm.includes(required)) failures.push(`pnpm workspace missing pattern: ${required}`);
  }
} else {
  failures.push("pnpm-workspace.yaml missing");
}

if (!readFileSync(".gitignore", "utf8").includes(".next-agent/")) {
  failures.push(".gitignore must ignore .next-agent/");
}

for (const toolchain of ["rust-toolchain.toml"]) {
  if (!existsSync(toolchain)) failures.push(`missing ${toolchain}`);
}

if (failures.length) {
  console.error(`Workspace check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(`Workspace check passed (${members.length} cargo members).`);
