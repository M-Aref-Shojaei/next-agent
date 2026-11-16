import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test, { beforeEach } from "node:test";
import {
  ALLOWED_FIELDS,
  FORBIDDEN_FIELDS,
  MAX_STRING,
  RATE_LIMIT,
  commandError,
  createLimiter,
  errorMessage,
  filterFields,
  installGlobalHandlers,
  log,
  redact,
  resetLogState,
  setLogSink,
} from "../src/lib/log.ts";

type Sent = { level: string; event: string; fields: Record<string, unknown> };
let sent: Sent[] = [];

beforeEach(() => {
  sent = [];
  resetLogState();
  setLogSink((level, event, fields) => void sent.push({ level, event, fields }));
});

test("keeps allowlisted fields and drops everything else", () => {
  log.info("provider_call", { provider: "anthropic", status: 200, ok: true, mystery: "x", prompt_len: 3 });
  assert.deepEqual(sent[0].fields, { provider: "anthropic", status: 200, ok: true });
  assert.equal(sent[0].event, "provider_call");
  assert.equal(sent[0].level, "info");
});

test("drops objects, arrays, null and non-finite numbers even under allowed names", () => {
  log.info("x", { error: { a: 1 }, files: [1], status: null, count: Number.NaN, bytes: 5 });
  assert.deepEqual(sent[0].fields, { bytes: 5 });
});

test("forbidden names never pass, and none is on the allowlist", () => {
  const fields = Object.fromEntries([...FORBIDDEN_FIELDS].map((n) => [n, "CANARY-PROMPT-7f3a"]));
  log.warn("leak_attempt", fields);
  assert.deepEqual(sent[0].fields, {});
  for (const name of FORBIDDEN_FIELDS) assert.equal(ALLOWED_FIELDS.has(name), false, name);
  assert.deepEqual(filterFields({ ...fields, count: 2 }), { count: 2 });
});

test("string values are redacted and cut", () => {
  const out = filterFields({
    error: "failed with sk-ant-api03-abcdefghijklmnop for sam@example.com in /Users/sam/work/notes.md",
  });
  assert.equal(out.error, "failed with [secret] for [redacted] in ~/work/notes.md");
  assert.equal(redact("Bearer abcdefghijklmnop1234"), "[secret]");
  assert.equal(redact("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw0"), "[secret]");
});

test("messages are cut to 300 characters", () => {
  assert.equal(errorMessage("word ".repeat(200)).length, MAX_STRING);
  assert.equal(errorMessage(new Error("short")), "short");
  log.error("command_failed", { error: "word ".repeat(200) });
  assert.equal((sent[0].fields.error as string).length, MAX_STRING);
});

test("limiter allows a burst up to the limit, then recovers after the window", () => {
  const limiter = createLimiter(3, 1000);
  assert.deepEqual([0, 1, 2, 3].map((t) => limiter.allow(t)), [true, true, true, false]);
  assert.equal(limiter.allow(999), false);
  assert.equal(limiter.allow(1001), true);
});

test("log calls past the rate limit are dropped, and the next event counts them", () => {
  const realNow = Date.now;
  let now = 1_000_000;
  Date.now = () => now;
  try {
    for (let i = 0; i < RATE_LIMIT.max + 5; i++) log.info("tick");
    assert.equal(sent.length, RATE_LIMIT.max);
    now += RATE_LIMIT.windowMs + 1;
    log.info("after");
    assert.equal(sent.length, RATE_LIMIT.max + 1);
    assert.equal(sent.at(-1)?.fields.retries, 5);
  } finally {
    Date.now = realNow;
  }
});

test("a failing sink never throws into the caller", async () => {
  setLogSink(() => {
    throw new Error("boom");
  });
  assert.doesNotThrow(() => log.error("x"));
  setLogSink(() => Promise.reject(new Error("later")));
  assert.doesNotThrow(() => log.error("y"));
  await new Promise((r) => setTimeout(r, 0));
});

test("global handlers log ui_error with kind, short message and page path, once", () => {
  const listeners = new Map<string, (e: unknown) => void>();
  const fake = {
    location: { pathname: "/settings/diagnostics" },
    addEventListener: (type: string, fn: (e: unknown) => void) => void listeners.set(type, fn),
  } as unknown as Window;
  installGlobalHandlers(fake);
  assert.equal(listeners.size, 2);
  installGlobalHandlers(fake); // second call does nothing
  listeners.get("error")?.({ error: new Error("word ".repeat(200)) });
  listeners.get("unhandledrejection")?.({ reason: new Error("nope") });
  // A bare string is a Rust command's error: it can name a page, so it is not sent.
  listeners.get("unhandledrejection")?.({ reason: 'board "CANARY-PROMPT-7f3a diary" not found' });
  assert.equal(sent[0].event, "ui_error");
  assert.equal(sent[0].fields.kind, "error");
  assert.equal(sent[0].fields.page, "/settings/diagnostics");
  assert.equal((sent[0].fields.error as string).length, MAX_STRING);
  assert.equal(sent[1].fields.kind, "unhandled_rejection");
  assert.equal(sent[1].fields.error, "nope");
  assert.deepEqual(sent[2].fields, { kind: "unhandled_rejection", page: "/settings/diagnostics" });
});

test("command_failed carries an error only for commands whose errors name no one's work", () => {
  const leak = 'a board named "CANARY-PROMPT-7f3a" already exists in /Users/sam/My Vault';
  for (const command of ["board_create", "graph_save_page", "graph_open", "skill_read", "memory_save", "design_source",
    "channel_save", "cron_save", "workspace_open", "vault_set", "review_action", "agent_profiles", "task_start", "some_new_command"]) {
    assert.equal(commandError(command, leak), undefined, command);
  }
  assert.equal(commandError("gateway_combo_save", new Error("unknown role `x`")), "unknown role `x`");
  assert.equal(commandError("setting_set", "router URL must be an http or https URL"), "router URL must be an http or https URL");
  assert.equal(commandError("gateway_settings_set", "key sk-ant-api03-abcdefghijklmnop"), "key [secret]");
});

/** `ui_error` kinds whose errors come from areas that never name people's work, plus the
 * global handlers' JavaScript exceptions (log.ts: an Error's message, never a command's string). */
const SAFE_UI_ERROR_KINDS = new Set([
  "app", "diagnostics", "providers", "settings", "upstreams", "usage", "overlay_updates",
  "error", "unhandled_rejection",
]);

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "node_modules" ? [] : sources(full);
    return /\.(ts|svelte)$/.test(entry.name) ? [full] : [];
  });
}

test("ui_error sends an error message only from areas whose errors name no one's work", () => {
  const root = join(import.meta.dirname, "..");
  const calls = [...sources(join(root, "src")), ...sources(join(root, "coucou/src"))].flatMap((file) =>
    [...readFileSync(file, "utf8").matchAll(/log\.\w+\("ui_error",\s*\{([^}]*)\}/g)].map((m) => ({ file, fields: m[1] })),
  );
  assert.ok(calls.length > 40, `found ${calls.length} ui_error calls`);
  for (const { file, fields } of calls) {
    const kind = /kind:\s*"([a-z_]+)"/.exec(fields)?.[1];
    if (kind === undefined || SAFE_UI_ERROR_KINDS.has(kind)) continue;
    assert.ok(!/\berror:/.test(fields), `${file}: ui_error kind ${kind} sends its error text`);
  }
});
