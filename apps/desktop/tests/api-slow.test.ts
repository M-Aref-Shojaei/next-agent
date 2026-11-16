import assert from "node:assert/strict";
import test from "node:test";
import { LONG_RUNNING_COMMANDS, isSlowCommand } from "../src/lib/api.ts";

// Commands that run for a long time by design (docs/plan/Logging System Plan.md, Wave 2 task 7).
const EXPECTED = [
  "task_start",
  "design_generate",
  "design_iterate",
  "design_convert",
  "design_implement",
  "gateway_catalog_refresh",
  "gateway_connection_test",
  "upstream_watch_check",
  "curator_run",
  "cron_run_now",
  "board_export",
  "graph_open",
];

test("the exempt set is exactly the long-running commands", () => {
  assert.deepEqual([...LONG_RUNNING_COMMANDS].sort(), [...EXPECTED].sort());
});

test("an exempt command is never reported as slow", () => {
  for (const command of EXPECTED) {
    assert.equal(isSlowCommand(command, 60_000), false, command);
  }
});

test("an ordinary command over 2 s is reported as slow", () => {
  assert.equal(isSlowCommand("workspace_list", 2001), true);
  assert.equal(isSlowCommand("setting_get", 9_000), true);
});

test("the 2 s threshold is exclusive and fast commands are not reported", () => {
  assert.equal(isSlowCommand("workspace_list", 2000), false);
  assert.equal(isSlowCommand("workspace_list", 12), false);
});
