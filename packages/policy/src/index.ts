// Reference implementation of the M1 decision table, used to (a) run the
// conformance corpora without a Rust toolchain and (b) drive UI previews of what
// a mode would do. It mirrors `crates/permission-engine`; the fixtures in
// `fixtures/` (policy-cases, command-corpus, path-corpus) are the shared
// contract. Change both together.
//
// The user-rule layer (allow/deny rules) is Rust-only: the preview shows the
// mode table, not a user's rule set.

import { analyze, autoForbidden, RANK } from "./command.ts";
import type { Analysis, CommandClass } from "./command.ts";
import { asciiLower, Checker, isSecretFile, lexical } from "./path.ts";
import type { FsFacts, PathStyle, Zone } from "./path.ts";

export { analyze, classify, splitSegments } from "./command.ts";
export type { Analysis, CommandClass, PathArg } from "./command.ts";
export { Checker, lexical, MapFs } from "./path.ts";
export type { FsFacts, PathCheck, PathStyle, Zone } from "./path.ts";

export type Mode = "plan" | "manual" | "auto" | "bypass";

export type Request =
  | { kind: "read"; path: string }
  | { kind: "list"; path: string }
  | { kind: "search"; path: string }
  | { kind: "write"; path: string }
  | { kind: "exec"; command: string; cwd?: string }
  | { kind: "network"; url: string }
  | { kind: "ask" };

export type Verdict = "allow" | "ask" | "deny";

export interface Context {
  root: string;
  home: string | null;
  workspaceId: string;
  caseInsensitive: boolean;
  allowlistedHosts: string[];
  pathStyle?: PathStyle;
}

export interface Evaluation {
  verdict: Verdict;
  rule: string;
  class?: CommandClass;
  zone?: Zone;
  resolved?: string | null;
}

// ---------------------------------------------------------------------------
// Modes (mirror of mode.rs)

export const BYPASS_CONFIRMATION_PHRASE = "bypass permissions";
export const BYPASS_CLI_FLAG = "--dangerously-skip-permissions";

/** `Shift+Tab`: plan → manual → auto → plan. Bypass cycles back to manual. */
export function cycleMode(mode: Mode): Mode {
  switch (mode) {
    case "plan":
    case "bypass":
      return "manual";
    case "manual":
      return "auto";
    default:
      return "plan";
  }
}

/** Restore a persisted mode; bypass and unknown values become `manual`. */
export function modeFromPersisted(name: string): Mode {
  return name === "plan" || name === "manual" || name === "auto" ? name : "manual";
}

export type ModeOrigin = "ui" | "cli" | "channel";

/** Whether a request to enter bypass carries the required consent. */
export function canEnterBypass(origin: ModeOrigin, proof: string): boolean {
  if (origin === "channel") return false;
  if (origin === "ui") return proof.trim() === BYPASS_CONFIRMATION_PHRASE;
  return proof === BYPASS_CLI_FLAG;
}

// ---------------------------------------------------------------------------
// Evaluation (mirror of engine.rs)

type Polarity = "allow" | "ask" | "deny";

interface Facts {
  zone: Zone | undefined;
  resolved: string | null | undefined;
  protected: boolean;
  secret: boolean;
  note: string | null;
  analysis: Analysis | undefined;
}

const ZONE_RANK: Record<Zone, number> = { inside: 0, outside: 1, sensitive: 2 };

function checkerFor(ctx: Context, fs: FsFacts): Checker {
  return new Checker(
    { root: ctx.root, home: ctx.home, pathStyle: ctx.pathStyle ?? "posix", caseInsensitive: ctx.caseInsensitive },
    fs,
  );
}

function gather(request: Request, ctx: Context, fs: FsFacts): Facts {
  const checker = checkerFor(ctx, fs);
  const facts: Facts = { zone: undefined, resolved: undefined, protected: false, secret: false, note: null, analysis: undefined };
  if ("path" in request) {
    const check = checker.check(null, request.path);
    facts.zone = check.zone;
    facts.note = check.note;
    facts.resolved = check.resolved;
    if (check.resolved !== null) {
      facts.protected = checker.isProtected(check.resolved);
      facts.secret = isSecretFile(check.resolved);
    }
  }
  if (request.kind === "exec") {
    const analysis = analyze(request.command);
    const cwdCheck = checker.check(null, request.cwd ?? ctx.root);
    let zone = cwdCheck.zone;
    facts.note = cwdCheck.note;
    for (const arg of analysis.paths) {
      const check = checker.checkShellWord(cwdCheck.resolved, arg.text);
      if (ZONE_RANK[check.zone] > ZONE_RANK[zone]) {
        zone = check.zone;
        facts.note = check.note;
      }
      if (check.resolved !== null && checker.isProtected(check.resolved)) facts.protected = true;
    }
    facts.zone = zone;
    facts.analysis = analysis;
  }
  return facts;
}

export function evaluate(request: Request, mode: Mode, ctx: Context, fs: FsFacts = lexical): Evaluation {
  const facts = gather(request, ctx, fs);
  const cls = facts.analysis?.class;
  const extra = { class: cls, zone: facts.zone, resolved: facts.resolved };

  if (cls === "catastrophic") return { verdict: "deny", rule: "catastrophic", ...extra };
  if (request.kind === "ask") return { verdict: "ask", rule: "ask_user", ...extra };

  const policy = modePolicy(request, mode, facts, ctx);
  return { verdict: policy, rule: policyRule(mode, policy), ...extra };
}

function modePolicy(request: Request, mode: Mode, facts: Facts, ctx: Context): Polarity {
  if (mode === "bypass") return "allow";
  const zone: Zone = facts.zone ?? "sensitive";
  switch (request.kind) {
    case "read":
    case "list":
    case "search":
      if (zone === "inside") return mode === "auto" && facts.secret ? "ask" : "allow";
      if (zone === "outside") return "ask";
      return mode === "auto" ? "deny" : "ask";
    case "write":
      if (mode === "plan") return "deny";
      if (mode === "auto") {
        if (zone === "inside") return facts.protected ? "ask" : "allow";
        return zone === "outside" ? "ask" : "deny";
      }
      return "ask";
    case "exec": {
      const cls: CommandClass = facts.analysis?.class ?? "unknown";
      if (mode === "plan") return "deny";
      if (mode === "auto") {
        if (autoForbidden(cls)) return "deny";
        if (zone === "sensitive") return "deny";
        if (cls === "safe_dev" && zone === "inside") return facts.protected ? "ask" : "allow";
      }
      return "ask";
    }
    case "network":
      if (mode === "plan") return "deny";
      if (mode === "auto" && hostAllowlisted(request.url, ctx.allowlistedHosts)) return "allow";
      return "ask";
    default:
      return "ask";
  }
}

function policyRule(mode: Mode, polarity: Polarity): string {
  if (polarity === "allow") return allowRule(mode);
  if (polarity === "ask") return askRule();
  return denyRule(mode);
}

/** Strict allowlist check; mirror of `host_allowlisted` in engine.rs. */
export function hostAllowlisted(url: string, hosts: string[]): boolean {
  if (/[\s\u0000-\u001f\u007f-\u009f\\%]/.test(url)) return false;
  const lower = asciiLower(url);
  let rest: string;
  if (lower.startsWith("https://")) rest = lower.slice(8);
  else if (lower.startsWith("http://")) rest = lower.slice(7);
  else return false;
  const authority = rest.split(/[/?#]/)[0] ?? "";
  if (authority.includes("@") || authority.startsWith("[")) return false;
  let host = authority;
  const colon = authority.lastIndexOf(":");
  if (colon !== -1) {
    const port = authority.slice(colon + 1);
    if (port === "" || !/^[0-9]+$/.test(port)) return false;
    host = authority.slice(0, colon);
  }
  if (host.endsWith(".")) host = host.slice(0, -1);
  return host !== "" && hosts.some((allowed) => asciiLower(allowed) === host);
}

export function classRank(cls: CommandClass): number {
  return RANK[cls];
}

/** Mirror of `reason_rule` in crates/permission-engine/src/engine.rs. */
function denyRule(mode: Mode): string {
  if (mode === "plan") return "plan_denied";
  if (mode === "auto") return "auto_forbidden";
  return "mode_denied";
}

function askRule(): string {
  return "mode_ask";
}

function allowRule(mode: Mode): string {
  switch (mode) {
    case "plan":
      return "plan_readonly";
    case "manual":
      return "mode_allow";
    case "auto":
      return "auto_sandbox";
    default:
      return "bypass";
  }
}

export function matchesFixtureCase(
  testCase: { mode: Mode; request: Record<string, unknown>; expect: Verdict; rule: string },
  ctx: Context,
  fs: FsFacts = lexical,
): { ok: boolean; actual: Evaluation } {
  const actual = evaluate(testCase.request as unknown as Request, testCase.mode, ctx, fs);
  return { ok: actual.verdict === testCase.expect && actual.rule === testCase.rule, actual };
}
