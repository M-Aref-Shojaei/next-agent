#!/usr/bin/env node
// Per-window command access (Tauri capabilities), generated from the code.
//
// One Tauri app runs Next Agent's main window and the overlay's windows (the
// vendored Coucou app, ADR-0011). `apps/desktop/src-tauri/build.rs` declares
// every command in an app manifest, so a window may call only the commands its
// capability grants. This script writes those capabilities:
//
//  * `main` gets Next Agent's own commands (the `generate_handler!` list in
//    apps/desktop/src-tauri/src/lib.rs), plus the overlay commands that
//    Settings → Overlay calls: the overlay's settings live in Next Agent's
//    Settings (owner request, ADR-0012), and apps/desktop/src/lib/
//    overlay-settings.ts is the one place in Next Agent's own code that may
//    name an overlay command.
//  * Each overlay window gets the overlay commands its own page can reach:
//    the modules imported from its entry point, the bridge methods those
//    modules call (core/bridge.ts, core/flow.ts, core/mail.ts) and the
//    commands those methods invoke. Never one of Next Agent's commands.
//
// `node scripts/gen-capabilities.mjs` rewrites the files; `--check` (run by
// `make check`) fails when they are out of date.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(import.meta.dirname, "..");
const APP = join(ROOT, "apps/desktop/src-tauri");
const OVERLAY = join(ROOT, "apps/desktop/coucou/src");
const CAPS = join(APP, "capabilities");

/**
 * Command names inside a `macro![ ... ]` call: the last path segment of each
 * entry. Brackets are matched and `#[...]` attributes skipped, as build.rs does.
 */
export function commandList(source, macro) {
  const start = source.indexOf(`${macro}[`);
  if (start < 0) throw new Error(`no ${macro}[ … ] in the source`);
  let body = "";
  let depth = 0;
  let attribute = 0;
  for (let i = start + macro.length + 1; i < source.length; i++) {
    const c = source[i];
    if (c === "#" && source[i + 1] === "[") {
      attribute = 1;
      i++;
      continue;
    }
    if (attribute > 0) {
      if (c === "[") attribute++;
      else if (c === "]") attribute--;
      continue;
    }
    if (c === "[") depth++;
    else if (c === "]" && depth === 0) break;
    else if (c === "]") depth--;
    body += c;
  }
  return body
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, "").trim())
    .flatMap((line) => line.split(","))
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => entry.split("::").pop());
}

export const NEXT_AGENT = commandList(readFileSync(join(APP, "src/lib.rs"), "utf8"), "generate_handler!");
export const OVERLAY_COMMANDS = commandList(
  readFileSync(join(ROOT, "apps/desktop/coucou/src-tauri/src/lib.rs"), "utf8"),
  "coucou_commands!",
);

/** The overlay's windows: label → its page's entry module. */
export const WINDOWS = {
  "coucou-island": "main.ts",
  // Next Agent: the Coucou window without its settings pages (they moved to Settings → Overlay).
  "coucou-settings": "hub/main.ts",
  "coucou-mochi": "desktop/main.ts",
  "coucou-pill": "pill/main.ts",
};

const BRIDGES = { Bridge: "core/bridge.ts", FlowBridge: "core/flow.ts", MailBridge: "core/mail.ts" };
const known = new Set(OVERLAY_COMMANDS);
const STRING = /"([a-z][a-z0-9_]*)"/g;
const INVOKE = /\b(?:invoke|call|callOrThrow)\s*(?:<[^()]*?>)?\s*\(\s*"([a-z][a-z0-9_]*)"/g;

/** Method → commands of one bridge object (`export const Name = { method: … }`). */
function bridgeMethods(file, name) {
  const text = readFileSync(join(OVERLAY, file), "utf8");
  const start = text.indexOf(`export const ${name} = {`);
  if (start < 0) throw new Error(`${file}: no export const ${name}`);
  const methods = new Map();
  let current = null;
  for (const line of text.slice(start).split("\n").slice(1)) {
    if (/^};?\s*$/.test(line)) break;
    const entry = /^ {2}(\w+)\s*[:(]/.exec(line);
    if (entry) {
      current = entry[1];
      methods.set(current, new Set());
    }
    if (!current) continue;
    for (const m of line.matchAll(STRING)) if (known.has(m[1])) methods.get(current).add(m[1]);
  }
  return methods;
}

const bridges = Object.fromEntries(Object.entries(BRIDGES).map(([name, file]) => [name, bridgeMethods(file, name)]));

/** Every overlay module reachable from `entry` through relative or `$coucou` imports. */
function reachable(entry) {
  const seen = new Set();
  const queue = [join(OVERLAY, entry)];
  while (queue.length) {
    const file = queue.pop();
    if (seen.has(file)) continue;
    seen.add(file);
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(/(?:from|import)\s*\(?\s*"([^"]+)"/g)) {
      let spec = m[1];
      if (spec.startsWith("$coucou/")) spec = join(OVERLAY, spec.slice(8));
      else if (spec.startsWith(".")) spec = resolve(dirname(file), spec);
      else continue;
      for (const candidate of [spec, `${spec}.ts`, join(spec, "index.ts")]) {
        if (candidate.endsWith(".ts") && existsSync(candidate)) {
          queue.push(candidate);
          break;
        }
      }
    }
  }
  return [...seen];
}

/** The overlay commands a window's page can call. */
export function windowCommands(entry) {
  const used = new Set();
  for (const file of reachable(entry)) {
    const text = readFileSync(file, "utf8");
    const isBridge = Object.values(BRIDGES).some((b) => file.endsWith(b));
    for (const [name, methods] of Object.entries(bridges)) {
      for (const m of text.matchAll(new RegExp(`\\b${name}\\.(\\w+)`, "g"))) {
        for (const cmd of methods.get(m[1]) ?? []) used.add(cmd);
      }
    }
    // Direct calls outside the bridges' method tables (voice.ts invokes one itself).
    if (!isBridge) for (const m of text.matchAll(INVOKE)) if (known.has(m[1])) used.add(m[1]);
  }
  return [...used].sort();
}

/** The file of Next Agent's own code that calls the overlay's settings commands. */
export const MAIN_OVERLAY_FILE = "apps/desktop/src/lib/overlay-settings.ts";

/** Every `.ts` / `.svelte` file of Next Agent's own frontend (not the overlay's mount points). */
function frontendFiles(dir = join(ROOT, "apps/desktop/src")) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (full !== join(ROOT, "apps/desktop/src/routes/coucou")) out.push(...frontendFiles(full));
    } else if (/\.(ts|svelte)$/.test(entry.name)) out.push(full);
  }
  return out;
}

/**
 * The overlay commands the main window may call: the ones overlay-settings.ts
 * invokes. Any other file of Next Agent's frontend naming one is an error, so
 * the grant can't grow without passing through that file.
 */
export function mainOverlayCommands() {
  const used = new Set();
  for (const file of frontendFiles()) {
    const text = readFileSync(file, "utf8");
    const named = [...text.matchAll(INVOKE)].map((m) => m[1]).filter((cmd) => known.has(cmd));
    if (named.length === 0) continue;
    if (file !== join(ROOT, MAIN_OVERLAY_FILE)) {
      throw new Error(`${file.slice(ROOT.length + 1)} calls overlay commands (${named.join(", ")}): go through ${MAIN_OVERLAY_FILE}`);
    }
    for (const cmd of named) used.add(cmd);
  }
  return [...used].sort();
}

const allow = (cmd) => `allow-${cmd.replaceAll("_", "-")}`;

function capability(identifier, description, windows, commands) {
  return `${JSON.stringify(
    {
      $schema: "../gen/schemas/desktop-schema.json",
      identifier,
      description,
      windows,
      permissions: ["core:default", ...commands.map(allow)],
    },
    null,
    2,
  )}\n`;
}

export function capabilities() {
  const clash = NEXT_AGENT.filter((c) => known.has(c));
  if (clash.length) throw new Error(`command names in both apps: ${clash.join(", ")}`);
  const files = {
    "default.json": capability(
      "default",
      `Main window: core IPC, Next Agent's own commands, and the overlay commands Settings → Overlay calls (${MAIN_OVERLAY_FILE}). Generated by scripts/gen-capabilities.mjs.`,
      ["main"],
      [...NEXT_AGENT].sort().concat(mainOverlayCommands()),
    ),
  };
  for (const [label, entry] of Object.entries(WINDOWS)) {
    const name = label.replace(/^coucou-/, "overlay-");
    files[`${name}.json`] = capability(
      name,
      `Overlay window ${label}: core IPC and the overlay commands its page (apps/desktop/coucou/src/${entry}) calls. None of Next Agent's. Generated by scripts/gen-capabilities.mjs.`,
      [label],
      windowCommands(entry),
    );
  }
  return files;
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] ?? "")) {
  const check = process.argv.includes("--check");
  const stale = [];
  for (const [file, text] of Object.entries(capabilities())) {
    const path = join(CAPS, file);
    const current = existsSync(path) ? readFileSync(path, "utf8") : null;
    if (current === text) continue;
    if (check) stale.push(file);
    else writeFileSync(path, text);
  }
  if (stale.length) {
    console.error(`Capabilities out of date: ${stale.join(", ")}. Run node scripts/gen-capabilities.mjs.`);
    process.exit(1);
  }
  console.log(check ? "Capabilities match the code." : "Capabilities written.");
}
