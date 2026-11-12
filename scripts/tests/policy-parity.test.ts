import { fileURLToPath } from "node:url";
// Confirms the parity guard fails when the Rust and TypeScript policy tables
// disagree — otherwise it is just a green light nobody trusts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, copyFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";

const root = fileURLToPath(new URL("../..", import.meta.url));
const script = join(root, "scripts/check-policy-parity.mjs");

function run() {
  return spawnSync(process.execPath, [script], { cwd: root, encoding: "utf8" });
}

test("the repository is in parity", () => {
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Policy parity check passed/);
});

test("a renamed TypeScript label is caught", () => {
  const target = join(root, "packages/policy/src/index.ts");
  const backup = join(root, "packages/policy/src/index.ts.parity-bak");
  copyFileSync(target, backup);
  try {
    const source = readFileSync(target, "utf8");
    writeFileSync(target, source.replace('return "auto_sandbox"', 'return "auto_sandbox_renamed"'));
    const result = run();
    assert.notEqual(result.status, 0, "renaming a label must fail the check");
    assert.match(result.stderr, /auto_sandbox/);
  } finally {
    copyFileSync(backup, target);
    unlinkSync(backup);
  }
});
