// Per-window command access: the overlay's windows never reach Next Agent's
// commands, the main window reaches exactly the overlay commands its
// Settings → Overlay pages call, and the committed capability files are what
// scripts/gen-capabilities.mjs makes of the code.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
// @ts-expect-error: a plain ES module without types
import { NEXT_AGENT, OVERLAY_COMMANDS, WINDOWS, capabilities, commandList, mainOverlayCommands, windowCommands } from "../gen-capabilities.mjs";

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

test("command lists are read past attributes and nested brackets", () => {
  const source = `x generate_handler![\n  a::one,\n  #[cfg(target_os = "macos")]\n  b::two, // note\n  three,\n] y`;
  assert.deepEqual(commandList(source, "generate_handler!"), ["one", "two", "three"]);
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

/**
 * Settings → Overlay (apps/desktop/src/lib/overlay-settings.ts): the overlay's
 * settings, moved from Coucou's own window into Next Agent's (owner request,
 * ADR-0012). Exactly these, and nothing the island or the chat needs.
 */
const MAIN_OVERLAY_ALLOWLIST = [
  // General, Updates
  "boot", "save_settings", "list_monitors", "custom_sounds", "reveal_sounds_folder", "reload_sounds", "open_url",
  "recap_prefs", "recap_set_enabled", "recap_clear",
  "update_status", "update_set_consent", "update_check", "update_set_source", "update_show", "update_open_page",
  // Agents: previews, and the writes that need their fingerprint
  "hooks_status", "agent_hooks_status", "hooks_preview", "hooks_apply", "agent_hooks_list", "agent_hooks_preview",
  "agent_hooks_apply", "agy_approvals_status", "agy_approvals_set_enabled", "status_line_preview", "status_line_apply",
  // Chat, integrations: keys are write-only, only their presence comes back
  "secret_present", "secret_set", "secret_clear", "claude_ready", "claude_api_models", "chat_models",
  "subscription_status", "subscription_set", "local_set_key", "local_connect",
  // Shortcuts
  "shortcuts_status", "shortcuts_suspend",
  // Phone link
  "phone_link_status", "phone_link_set_enabled", "phone_link_pairing", "phone_link_new_pairing",
  "phone_details_status", "phone_details_set_enabled", "phone_answers_status", "phone_answers_set_enabled",
  "phone_diffs_status", "phone_diffs_set_enabled", "phone_services_status", "phone_services_set",
  "phone_relay_status", "phone_relay_set", "phone_relay_set_access", "phone_usage_status", "phone_usage_set_enabled",
  "phone_recap_status", "phone_recap_set_enabled", "phone_chat_status", "phone_chat_set_enabled", "phone_chat_set_models",
  // Dictation, Notetaker, voice
  "flow_status", "flow_set_prefs", "flow_clear", "flow_styles", "flow_set_styles", "flow_running_apps",
  "flow_paste_again", "flow_models_download", "flow_models_remove",
].sort();

test("the main window may call exactly the overlay's settings commands", () => {
  const overlay = new Set<string>(OVERLAY_COMMANDS.map(allow));
  const granted = forWindow("main")[0].permissions.filter((p) => overlay.has(p));
  assert.deepEqual([...granted].sort(), MAIN_OVERLAY_ALLOWLIST.map(allow).sort());
  assert.deepEqual(mainOverlayCommands(), MAIN_OVERLAY_ALLOWLIST);
  // Never what only the island or the chat does.
  for (const cmd of ["approval_decision", "approval_answer", "chat_send", "quit_app", "phone_link_publish", "set_collapsed", "launch_session"]) {
    assert.ok(!granted.includes(allow(cmd)), cmd);
  }
});

test("the Coucou window kept its Flow pages and lost the settings it no longer shows", () => {
  const hub = forWindow("coucou-settings")[0].permissions;
  for (const cmd of ["flow_history", "notes_state", "notes_export", "open_settings_window"]) assert.ok(hub.includes(allow(cmd)), cmd);
  for (const cmd of ["hooks_apply", "status_line_apply", "agent_hooks_apply", "secret_set", "secret_clear", "local_set_key", "save_settings", "phone_link_set_enabled", "agy_approvals_set_enabled", "update_set_source"]) {
    assert.ok(!hub.includes(allow(cmd)), cmd);
  }
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
  // One exception: the Design Studio frames its preview from the loopback proxy.
  assert.deepEqual(rules.get("frame-src"), ["'self'", "http://127.0.0.1:*"]);
  const rest = csp.replace(/frame-src [^;]*/, "");
  assert.ok(!/unsafe-eval|https?:\/\/(?!ipc\.localhost)|\*/.test(rest), csp);
});
