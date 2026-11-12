// Project topology generator: manifest edges, probe-gated links, rendering.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { collect, renderMermaid, renderNote } from "../lib/topology.mjs";

function repo(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "topology-"));
  for (const [rel, body] of Object.entries(files)) {
    mkdirSync(join(root, rel, ".."), { recursive: true });
    writeFileSync(join(root, rel), body);
  }
  return root;
}

const workspace = (...members: string[]) =>
  `[workspace]\nmembers = [\n${members.map((m) => `  "${m}",\n`).join("")}]\n`;

const base = {
  "Cargo.toml": workspace("crates/core", "crates/permission-engine"),
  "crates/core/Cargo.toml":
    '[package]\nname = "next-agent-core"\n\n[dependencies]\nserde = "1"\nnext-agent-permission-engine = { path = "../permission-engine" }\n\n[dev-dependencies]\ntempfile = "3"\n',
  "crates/permission-engine/Cargo.toml": '[package]\nname = "next-agent-permission-engine"\n',
  "packages/contracts/package.json": '{ "name": "@next-agent/contracts" }',
  "apps/desktop/package.json":
    '{ "name": "@next-agent/desktop", "dependencies": { "@next-agent/contracts": "workspace:*", "svelte": "5" } }',
  "services/agent-gateway/README.md": "# agent-gateway\n\nRuns external engines.\n",
};

test("edges come from manifests; external crates and npm deps are ignored", () => {
  const topology = collect(repo(base));
  const edges = topology.edges.map((e) => `${e.from}->${e.to}:${e.kind}`);
  assert.deepEqual(edges.sort(), [
    "@next-agent/desktop->@next-agent/contracts:normal",
    "next-agent-core->next-agent-permission-engine:normal",
  ]);
  const planned = topology.projects.find((p) => p.dir === "services/agent-gateway");
  assert.equal(planned?.lang, "planned");
});

test("cross-language links appear only while their probe holds", () => {
  const without = collect(repo(base));
  assert.ok(!without.edges.some((e) => e.kind === "ipc"));
  assert.deepEqual(without.externals, []);

  const withLinks = collect(
    repo({
      ...base,
      "Cargo.toml": workspace("crates/core", "crates/permission-engine", "apps/desktop/src-tauri"),
      "apps/desktop/src-tauri/Cargo.toml":
        '[package]\nname = "next-agent-desktop"\n\n[dependencies]\nnext-agent-core = { path = "../../../crates/core" }\n',
      "apps/desktop/src/api.ts": 'invoke("x")',
      "crates/core/src/worktree.rs": 'Command::new("git")',
    }),
  );
  assert.ok(withLinks.edges.some((e) => e.kind === "ipc" && e.to === "next-agent-desktop"));
  assert.deepEqual(withLinks.externals, ["ext:git"]);
});

test("rendering is deterministic and labels both halves of an app", () => {
  const root = repo({
    ...base,
    "Cargo.toml": workspace("crates/core", "crates/permission-engine", "apps/desktop/src-tauri"),
    "apps/desktop/src-tauri/Cargo.toml": '[package]\nname = "next-agent-desktop"\n',
  });
  const a = renderNote(collect(root));
  assert.equal(a, renderNote(collect(root)));
  const mermaid = renderMermaid(collect(root));
  assert.match(mermaid, /^flowchart LR/);
  assert.match(mermaid, /desktop backend \(Tauri\)/);
  assert.match(mermaid, /desktop UI/);
  assert.match(mermaid, /next_agent_core --> next_agent_permission_engine/);
});
