// Tests the log-field checker (scripts/check-log-fields.mjs, ADR-0013): it must
// find every field of tracing macros and `log.*` calls, reject forbidden
// names outright, and hold non-allowlisted names to the pending allowlist.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  applyAllowlist,
  checkCalls,
  isForbidden,
  readFieldLists,
  scanRust,
  scanTs,
  stripComments,
} from "../lib/log-fields.mjs";

const ROOT = resolve(import.meta.dirname, "../..");
const FIELDS = join(ROOT, "crates/diagnostics/src/fields.rs");
const lists = readFieldLists(readFileSync(FIELDS, "utf8"));
const names = (src: string) => scanRust(src).map((c: { fields: string[] }) => c.fields);

test("the lists come from fields.rs and do not overlap", () => {
  assert.ok(lists.allowed.has("duration_ms"));
  assert.ok(lists.allowed.has("task_id"));
  assert.ok(lists.forbidden.has("prompt"));
  assert.ok(lists.forbidden.has("token"));
  for (const name of lists.forbidden) assert.ok(!lists.allowed.has(name), name);
});

test("Rust: every field form is found, the message and format args are not", () => {
  const src = `
    tracing::warn!(target: "router", %error, call_id, provider = %p.label(), "x" = 1, status = ?s, "failed {}", path);
    info!(seq = e.seq, kind = e.kind.kind(), %self.prompt, ?root.path);
    tracing::event!(tracing::Level::INFO, count = 3, "done");
    let span = tracing::info_span!("task", task_id = %id, title);
  `;
  assert.deepEqual(names(src), [
    ["error", "call_id", "provider", "x", "status"],
    ["seq", "kind", "self.prompt", "root.path"],
    ["count"],
    ["task_id", "title"],
  ]);
});

test("Rust: multi-line calls, nested parens, strings with commas and comments", () => {
  const src = `
    // tracing::info!(prompt = %x, "commented out");
    /* warn!(body = b) */
    tracing::info!(
        target: "router",
        model = %format!("{}, {}", a, b), // trailing comment, with a comma
        latency_ms = u64::try_from(x.as_millis()).unwrap_or(u64::MAX),
        "router chat completed"
    );
    let s = "error!(token = t)";
  `;
  assert.deepEqual(names(src), [["model", "latency_ms"]]);
});

test("Rust: other crates' macros are ignored", () => {
  assert.deepEqual(names(`log::info!("x {}", prompt); anyhow::bail!("e"); my::error!(prompt);`), []);
});

test("a planted tracing::info!(prompt = %x, …) is a forbidden-field failure", () => {
  const problems = checkCalls(scanRust(`fn f(x: &str) { tracing::info!(prompt = %x, "sent"); }`), lists);
  assert.equal(problems.length, 1);
  assert.equal(problems[0].forbidden, true);
  assert.match(problems[0].why, /forbidden field "prompt"/);
  // Even a generous pending entry does not cover a forbidden name.
  const results = new Map([["crates/x/src/lib.rs", problems]]);
  const { failures } = applyAllowlist(results, [{ path: "crates/x/src/lib.rs", max: 99 }]);
  assert.ok(failures.some((f: string) => f.includes('forbidden field "prompt"')));
});

test("forbidden names are caught alone and as dotted segments only", () => {
  for (const name of ["prompt", "content", "title", "path", "token", "key", "email", "self.body", "req.authorization"]) {
    assert.ok(isForbidden(name, lists), name);
  }
  for (const name of ["prompt_tokens", "tokens_in", "duration_ms", "task_id"]) {
    assert.ok(!isForbidden(name, lists), name);
  }
});

test("allowlisted and own names pass; unknown ones are pending problems", () => {
  const calls = scanRust(`tracing::info!(task_id, duration_ms = 3, event = "x", message = "m", trips = 2, "y");`);
  const problems = checkCalls(calls, lists);
  assert.deepEqual(problems.map((p: { why: string; forbidden: boolean }) => [p.forbidden, p.why]), [
    [false, 'field "trips" is not on the allowlist (crates/diagnostics/src/fields.rs)'],
  ]);
});

test("TypeScript: log.* calls are checked for event names, literal fields and keys", () => {
  const src = `
    log.info("command_failed", { command, duration_ms: ms, "error": redact(e) });
    log.warn("Bad Name", { prompt: text });
    log.error(name, fields);
    log.info("ui_error", { ...rest, [k]: v, page: "x" });
    log.debug("ok_event");
    // log.error("commented", { body });
  `;
  const calls = scanTs(src);
  assert.equal(calls.length, 5);
  assert.deepEqual(calls[0].fields, ["command", "duration_ms", "error"]);
  assert.deepEqual(calls[0].problems, []);
  const problems = checkCalls(calls, lists).map((p: { line: number; why: string }) => `${p.line}: ${p.why}`);
  assert.deepEqual(problems, [
    '3: event "Bad Name" is not a snake_case name',
    '3: forbidden field "prompt" (user content or a credential)',
    "4: the event name must be a string literal",
    "4: fields must be an object literal",
    "5: fields must not be spread",
    "5: computed field key [k]",
  ]);
});

test("comments are blanked but strings, char literals and line numbers stay", () => {
  const src = `let a = '"'; // x\nlet u = "http://x"; /* y\n z */ let r = r#"a // b"#;`;
  const out = stripComments(src);
  assert.equal(out.split("\n").length, src.split("\n").length);
  assert.ok(out.includes(`'"'`));
  assert.ok(out.includes(`"http://x"`));
  assert.ok(out.includes(`r#"a // b"#`));
  assert.ok(!out.includes("// x"));
  assert.ok(!out.includes("y"));
});

test("pending allowlist: counts may only go down, clean files must be removed", () => {
  const p = (forbidden = false) => ({ line: 1, why: "w", forbidden });
  const results = new Map([
    ["a.rs", [p(), p()]],
    ["b.rs", [p()]],
    ["c.rs", [p(), p(), p()]],
    ["d.rs", []],
  ]);
  const allow = [
    { path: "a.rs", max: 2 },
    { path: "b.rs", max: 3 },
    { path: "c.rs", max: 2 },
    { path: "d.rs", max: 1 },
    { path: "gone.rs", max: 1 },
  ];
  const { failures, notes } = applyAllowlist(results, allow);
  assert.deepEqual(failures, [
    "c.rs: 3 non-allowlisted log fields, pending allowlist max is 2 (no new ones)",
    "d.rs: in the log-fields allowlist but now clean; remove the entry",
    "gone.rs: in the log-fields allowlist but not found; remove the entry",
  ]);
  assert.equal(notes.length, 1);
  assert.match(notes[0], /b\.rs: down to 1/);
});

test("check-log-fields.mjs exits non-zero on a planted forbidden field", () => {
  const dir = mkdtempSync(join(tmpdir(), "log-fields-"));
  mkdirSync(join(dir, "crates/diagnostics/src"), { recursive: true });
  mkdirSync(join(dir, "crates/planted/src"), { recursive: true });
  mkdirSync(join(dir, "scripts"), { recursive: true });
  copyFileSync(FIELDS, join(dir, "crates/diagnostics/src/fields.rs"));
  writeFileSync(join(dir, "scripts/log-fields-allowlist.json"), JSON.stringify({ entries: [] }));
  writeFileSync(join(dir, "crates/planted/src/lib.rs"), `pub fn f(x: &str) {\n    tracing::info!(prompt = %x, "sent");\n}\n`);
  const run = spawnSync(process.execPath, [join(ROOT, "scripts/check-log-fields.mjs")], { cwd: dir, encoding: "utf8" });
  assert.equal(run.status, 1, run.stdout + run.stderr);
  assert.match(run.stderr, /crates\/planted\/src\/lib\.rs:2: forbidden field "prompt"/);
});

test("the repository passes the check as committed", () => {
  const run = spawnSync(process.execPath, [join(ROOT, "scripts/check-log-fields.mjs")], { cwd: ROOT, encoding: "utf8" });
  assert.equal(run.status, 0, run.stdout + run.stderr);
});
