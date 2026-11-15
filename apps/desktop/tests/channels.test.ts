import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_TOOLS,
  SELECTABLE_TOOLS,
  expiresIn,
  formErrors,
  healthLabel,
  looksLikeToken,
  newChannel,
  parseIds,
  toggleTool,
  type PairingRequest,
} from "../src/lib/channels.ts";
import { messageTaskId } from "../src/lib/timeline.ts";

const TOKEN = "123456:ABCdefGHIjklMNOpqrSTUvwxYZ";

test("new channels start with the strict defaults", () => {
  const c = newChannel("ws_1");
  assert.equal(c.mode, "manual");
  assert.equal(c.dmPolicy, "pairing");
  assert.equal(c.groupPolicy, "disabled");
  assert.equal(c.dmScope, "per-channel-peer");
  assert.deepEqual(c.tools, DEFAULT_TOOLS);
  assert.ok(!c.tools.includes("run_command"));
});

test("form errors: token, workspace and the explicit * for open", () => {
  const c = newChannel("");
  assert.equal(formErrors(c, [], "", false).length, 2);
  assert.deepEqual(formErrors({ ...c, workspaceId: "ws" }, [], TOKEN, false), []);
  assert.deepEqual(formErrors({ ...c, workspaceId: "ws" }, [], "", true), [], "saved token is enough");
  assert.equal(formErrors({ ...c, workspaceId: "ws" }, [], "nope", true).length, 1);
  const open = { ...c, workspaceId: "ws", dmPolicy: "open" as const };
  assert.equal(formErrors(open, [], "", true).length, 1);
  assert.deepEqual(formErrors(open, [{ senderId: "*", label: "", source: "manual", addedAt: "" }], "", true), []);
  const sneaky = { ...c, workspaceId: "ws", mode: "bypassPermissions" as unknown as "manual" };
  assert.equal(formErrors(sneaky, [], "", true).length, 1);
});

test("token shape", () => {
  assert.ok(looksLikeToken(TOKEN));
  assert.ok(!looksLikeToken("abc"));
  assert.ok(!looksLikeToken("123:short"));
});

test("health labels", () => {
  assert.equal(healthLabel(null, false).text, "Disabled");
  assert.equal(healthLabel({ state: "connected", detail: "", restarts: 0, since: 0 }, true).tone, "ok");
  const failed = healthLabel({ state: "failed", detail: "bad token", restarts: 0, since: 0 }, true);
  assert.equal(failed.tone, "error");
  assert.match(failed.text, /bad token/);
});

test("pairing expiry label", () => {
  const p: PairingRequest = { channelId: "c", senderId: "1", chatId: "1", label: "@a", code: "X", createdAt: 1000, lastSeenAt: 1000 };
  assert.equal(expiresIn(p, 1000), "expires in 60 min");
  assert.equal(expiresIn(p, 1000 + 3600), "expired");
});

test("ids and tool toggles", () => {
  assert.deepEqual(parseIds("-100, -200\n-100  "), ["-100", "-200"]);
  assert.deepEqual(toggleTool(["read_file"], "run_command", true), ["read_file", "run_command"]);
  assert.deepEqual(toggleTool(["read_file", "edit"], "read_file", false), ["edit"]);
  assert.deepEqual(toggleTool([], "subagent", true), [], "unknown tools are dropped");
  assert.equal(SELECTABLE_TOOLS.length, 8);
});

test("stream messages name their task", () => {
  assert.equal(messageTaskId({ type: "notice", task_id: "t1", tone: "info", text: "x" }), "t1");
  assert.equal(
    messageTaskId({ type: "event", event: { v: 1, id: "t2:0", task_id: "t2", seq: 0, ts: "", kind: "message", data: {} } }),
    "t2",
  );
});
