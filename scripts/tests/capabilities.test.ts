// Per-window command access: the overlay's windows never reach Next Agent's
// commands, the main window never reaches the overlay's, and the committed
// capability files are what scripts/gen-capabilities.mjs makes of the code.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
// @ts-expect-error: a plain ES module without types
import { NEXT_AGENT, OVERLAY_COMMANDS, WINDOWS, capabilities, windowCommands } from "../gen-capabilities.mjs";

const root = fileURLToPath(new URL("../..", import.meta.url));
const dir = join(root, "apps/desktop/src-tauri/capabilities");
const allow = (cmd: string) => `allow-${cmd.replaceAll("_", "-")}`;
type Capability = { identifier: string; windows: string[]; permissions: string[] };
const files: Capability[] = readdirSync(dir).map((f) => JSON.parse(readFileSync(join(dir, f), "utf8")));
const forWindow = (label: string) => files.filter((c) => c.windows.includes(label));

test("the committed capabilities are the generated ones", () => {
  const generated = capabilities() as Record<string, string>;
  assert.deepEqual(readdirSync(dir).sort(), Object.keys(generated).sort());
  for (const [file, text] of Object.entries(generated)) assert.equal(readFileSync(join(dir, file), "utf8"), text, file);
});

test("each window has exactly one capability", () => {
  for (const label of ["main", ...Object.keys(WINDOWS)]) assert.equal(forWindow(label).length, 1, label);
});

test("no overlay window may call any of Next Agent's commands", () => {
  const ours = new Set<string>(NEXT_AGENT.map(allow));
  for (const label of Object.keys(WINDOWS)) {
    const granted = forWindow(label)[0].permissions;
    assert.deepEqual(granted.filter((p) => ours.has(p)), [], label);
    // The ones that matter most, by name.
    for (const cmd of ["rule_delete", "setting_set", "vault_set", "gateway_connection_save", "gateway_settings_set", "approval_respond", "task_start"]) {
      assert.ok(!granted.includes(allow(cmd)), `${label}: ${cmd}`);
    }
  }
});

test("the main window may call none of the overlay's commands", () => {
  const overlay = new Set<string>(OVERLAY_COMMANDS.map(allow));
  assert.deepEqual(forWindow("main")[0].permissions.filter((p) => overlay.has(p)), []);
});

test("each overlay window gets only what its page calls", () => {
  // The island answers approvals but cannot install hooks, store keys or change Settings-only switches.
  const island = forWindow("coucou-island")[0].permissions;
  for (const cmd of ["approval_decision", "approval_ack", "boot"]) assert.ok(island.includes(allow(cmd)), cmd);
  for (const cmd of ["hooks_apply", "status_line_apply", "agent_hooks_apply", "secret_set", "secret_clear", "local_set_key", "phone_link_set_enabled", "update_set_source", "quit_app"]) {
    assert.ok(!island.includes(allow(cmd)), cmd);
  }
  // The companion and the pill are small.
  assert.ok(windowCommands(WINDOWS["coucou-mochi"]).length < 12);
  assert.ok(windowCommands(WINDOWS["coucou-pill"]).length < 15);
});

test("the CSP stays strict: local only, no inline or eval'd script, no plugins", () => {
  const conf = JSON.parse(readFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  const csp: string = conf.app.security.csp;
  const rules = new Map(csp.split(";").map((r) => r.trim().split(/\s+/)).map(([k, ...v]) => [k, v]));
  assert.deepEqual(rules.get("default-src"), ["'self'"]);
  assert.ok(!rules.has("script-src"), "scripts fall back to default-src 'self'");
  assert.deepEqual(rules.get("object-src"), ["'none'"]);
  assert.deepEqual(rules.get("frame-ancestors"), ["'none'"]);
  assert.ok(!/unsafe-eval|https?:\/\/(?!ipc\.localhost)|\*/.test(csp), csp);
});
