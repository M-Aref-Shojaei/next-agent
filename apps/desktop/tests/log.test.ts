import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";
import {
  ALLOWED_FIELDS,
  FORBIDDEN_FIELDS,
  MAX_STRING,
  RATE_LIMIT,
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
  listeners.get("unhandledrejection")?.({ reason: "nope" });
  assert.equal(sent[0].event, "ui_error");
  assert.equal(sent[0].fields.kind, "error");
  assert.equal(sent[0].fields.page, "/settings/diagnostics");
  assert.equal((sent[0].fields.error as string).length, MAX_STRING);
  assert.equal(sent[1].fields.kind, "unhandled_rejection");
  assert.equal(sent[1].fields.error, "nope");
});
