import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  canEnterBypass,
  Checker,
  classify,
  cycleMode,
  evaluate,
  hostAllowlisted,
  MapFs,
  modeFromPersisted,
} from "../src/index.ts";
import type { Context, Mode, PathStyle, Request } from "../src/index.ts";

type Json = Record<string, unknown>;

function fixture(name: string): Json {
  return JSON.parse(readFileSync(new URL(`../../../fixtures/${name}`, import.meta.url), "utf8")) as Json;
}

function merged(base: Json, overrides: unknown): Json {
  return { ...base, ...((overrides as Json | undefined) ?? {}) };
}

function context(value: Json): Context {
  return {
    root: value.root as string,
    home: (value.home as string | null | undefined) ?? null,
    workspaceId: (value.workspaceId as string | undefined) ?? "ws_1",
    caseInsensitive: (value.caseInsensitive as boolean | undefined) ?? false,
    allowlistedHosts: (value.allowlistedHosts as string[] | undefined) ?? [],
    pathStyle: (value.pathStyle as PathStyle | undefined) ?? "posix",
  };
}

function filesystem(value: Json): MapFs {
  const fs = new MapFs((value.pathStyle as PathStyle | undefined) ?? "posix");
  const spec = (value.fs as { entries?: string[]; links?: Record<string, string> } | undefined) ?? {};
  for (const entry of spec.entries ?? []) fs.withEntry(entry);
  for (const [link, target] of Object.entries(spec.links ?? {})) fs.withLink(link, target);
  return fs;
}

const policy = fixture("policy-cases.json");
const policyCases = policy.cases as { name: string; mode: Mode; request: Json; expect: string; rule: string; context?: Json }[];

test("policy fixture covers the decision table", () => {
  assert.ok(policyCases.length >= 60);
});

for (const testCase of policyCases) {
  test(`policy: ${testCase.name}`, () => {
    const ctxValue = merged(policy.context as Json, testCase.context);
    const request = { ...testCase.request } as Json;
    if (request.kind === "exec" && request.cwd === undefined) request.cwd = ctxValue.root;
    const actual = evaluate(request as unknown as Request, testCase.mode, context(ctxValue), filesystem(ctxValue));
    assert.equal(`${actual.verdict}/${actual.rule}`, `${testCase.expect}/${testCase.rule}`, testCase.name);
  });
}

const commands = fixture("command-corpus.json");
const commandCases = commands.cases as { command: string; class: string }[];

test("command corpus is substantial", () => {
  assert.ok(commandCases.length >= 200);
});

test("command corpus classifies identically", () => {
  const failures = commandCases
    .map((c) => ({ ...c, actual: classify(c.command) }))
    .filter((c) => c.actual !== c.class)
    .map((c) => `${JSON.stringify(c.command)}: expected ${c.class}, got ${c.actual}`);
  assert.deepEqual(failures, []);
});

const paths = fixture("path-corpus.json");
const pathCases = paths.cases as { path: string; zone: string; cwd?: string; shell?: boolean; resolved?: string | null; context?: Json }[];

test("path corpus has at least 50 escape cases", () => {
  assert.ok(pathCases.length >= 50);
});

test("path corpus resolves identically", () => {
  const failures: string[] = [];
  for (const c of pathCases) {
    const ctxValue = merged(paths.context as Json, c.context);
    const ctx = context(ctxValue);
    const checker = new Checker(
      { root: ctx.root, home: ctx.home, pathStyle: ctx.pathStyle ?? "posix", caseInsensitive: ctx.caseInsensitive },
      filesystem(ctxValue),
    );
    const check = c.shell ? checker.checkShellWord(c.cwd ?? null, c.path) : checker.check(c.cwd ?? null, c.path);
    const problems: string[] = [];
    if (check.zone !== c.zone) problems.push(`zone ${check.zone}`);
    if (c.resolved !== undefined && check.resolved !== c.resolved) problems.push(`resolved ${JSON.stringify(check.resolved)}`);
    if (problems.length) failures.push(`${JSON.stringify(c.path)} (expected ${c.zone}): ${problems.join(", ")}`);
  }
  assert.deepEqual(failures, []);
});

test("mode cycle never reaches bypass", () => {
  let mode: Mode = "manual";
  for (let i = 0; i < 9; i += 1) {
    mode = cycleMode(mode);
    assert.notEqual(mode, "bypass");
  }
  assert.equal(cycleMode("bypass"), "manual");
  assert.equal(cycleMode("plan"), "manual");
  assert.equal(cycleMode("manual"), "auto");
  assert.equal(cycleMode("auto"), "plan");
  assert.equal(modeFromPersisted("bypass"), "manual");
  assert.equal(modeFromPersisted("auto"), "auto");
  assert.ok(canEnterBypass("ui", " bypass permissions "));
  assert.ok(!canEnterBypass("ui", "bypass"));
  assert.ok(canEnterBypass("cli", "--dangerously-skip-permissions"));
  assert.ok(!canEnterBypass("channel", "bypass permissions"));
});

test("host allowlist is strict", () => {
  const hosts = ["docs.rs"];
  for (const url of ["https://docs.rs/x", "http://DOCS.RS", "https://docs.rs:443/", "https://docs.rs./"]) {
    assert.ok(hostAllowlisted(url, hosts), url);
  }
  for (const url of [
    "https://docs.rs@evil.com/",
    "https://evil.com\\@docs.rs/",
    "https://docs.rs.evil.com/",
    "ftp://docs.rs/",
    "docs.rs/x",
    "https://docs.rs%2F@evil.com",
    "https://docs.rs:evil/",
    "https://[::1]/",
    " https://docs.rs/",
    "https://docs.r\ns/",
  ]) {
    assert.ok(!hostAllowlisted(url, hosts), url);
  }
});
