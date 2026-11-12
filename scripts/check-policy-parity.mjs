#!/usr/bin/env node
// The policy table lives twice: once in Rust (`crates/permission-engine`) and
// once in TypeScript (`packages/policy`, used by the UI preview). The shared
// fixture checks behaviour, but only for the cases someone remembered to write.
// This checks the vocabulary itself — rule labels and tool names — so a rename
// in one language cannot silently break the other.
import { readFileSync } from "node:fs";

const failures = [];

// `packages/policy` previews the mode table only; it has no user-rule layer.
// These labels are therefore Rust-only by design, not drift.
const RUST_ONLY_LABELS = new Set(["user_deny", "user_allow"]);

function section(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  if (start === -1) return "";
  const end = source.indexOf(endMarker, start + startMarker.length);
  return source.slice(start, end === -1 ? undefined : end);
}

function quoted(source) {
  return new Set([...source.matchAll(/"([a-z_]+)"/g)].map((match) => match[1]));
}

// Rust match arms yield bare literals (`Mode::Plan => "plan_readonly"`);
// TypeScript uses `return "plan_readonly"`. Matching only those avoids picking
// up `case "plan":` labels.
function rustArms(source) {
  return new Set([...source.matchAll(/=>\s*"([a-z_]+)"/g)].map((match) => match[1]));
}

// Early returns name their label inline: `build(..., "catastrophic")`.
function rustLabelArgs(source) {
  return new Set([...source.matchAll(/"([a-z_]+)"\s*,?\s*\)/g)].map((match) => match[1]));
}

function tsReturns(source) {
  return new Set([...source.matchAll(/return\s+"([a-z_]+)"/g)].map((match) => match[1]));
}

// Labels also appear as optimistic `rule: "..."` fields on evaluation objects.
function tsRuleFields(source) {
  return new Set([...source.matchAll(/rule:\s*"([a-z_]+)"/g)].map((match) => match[1]));
}

function diff(name, rust, ts) {
  const onlyRust = [...rust].filter((value) => !ts.has(value) && !RUST_ONLY_LABELS.has(value));
  const onlyTs = [...ts].filter((value) => !rust.has(value));
  if (onlyRust.length) failures.push(`${name}: only in Rust: ${onlyRust.join(", ")}`);
  if (onlyTs.length) failures.push(`${name}: only in TypeScript: ${onlyTs.join(", ")}`);
}

// 1. Rule labels returned by `reason_rule` vs `allowRule`/`denyRule`.
const rustEngine = readFileSync("crates/permission-engine/src/engine.rs", "utf8");
const tsPolicy = readFileSync("packages/policy/src/index.ts", "utf8");

const rustLabels = new Set([
  ...rustArms(section(rustEngine, "fn reason_rule", "\nfn mode_policy")),
  ...rustLabelArgs(section(rustEngine, "fn explain", "\nfn resolve")),
]);
const tsLabels = new Set([
  ...tsReturns(section(tsPolicy, "function denyRule", "\nexport function matchesFixtureCase")),
  ...tsRuleFields(tsPolicy),
]);
if (rustLabels.size === 0 || tsLabels.size === 0) {
  failures.push("could not extract rule labels from one side; check the markers");
} else {
  diff("rule labels", rustLabels, tsLabels);
}

// 2. Tool names: Rust `ToolRequest::kind` is the canonical spelling.
const rustTools = quoted(section(readFileSync("crates/core/src/tools.rs", "utf8"), "pub fn kind", "\n    }"));
const rustPolicyTools = quoted(
  section(rustEngine, "pub fn tool(&self)", "\n    }"),
);
if (rustTools.size !== 7) failures.push(`expected 7 tool names in tools.rs, found ${rustTools.size}`);
diff("tool names (policy crate vs tool layer)", rustTools, rustPolicyTools);

// 3. Every event kind the runner emits must have a fixture-agnostic wire name.
const rustEvents = quoted(section(readFileSync("crates/core/src/events.rs", "utf8"), "pub fn kind", "\n    }"));
for (const required of ["tool_requested", "permission_decided", "task_completed", "task_failed"]) {
  if (!rustEvents.has(required)) failures.push(`events.rs is missing kind "${required}"`);
}

if (failures.length) {
  console.error(`Policy parity check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(
  `Policy parity check passed (${rustLabels.size} rule labels, ${rustTools.size} tool names, ${rustEvents.size} event kinds).`,
);
