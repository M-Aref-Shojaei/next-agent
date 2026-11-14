// Project topology: which crate / package / service depends on which.
// Reads manifests only (no cargo, no pnpm install needed), so it runs anywhere
// `make check` does. Rendered as a Mermaid diagram in the docs vault.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const read = (root, rel) => readFileSync(join(root, rel), "utf8");
const exists = (root, rel) => existsSync(join(root, rel));

/** Recursively test whether any file under `rel` matching `ext` contains `needle`. */
function sourceContains(root, rel, needle, ext = /\.(rs|ts|svelte)$/) {
  const dir = join(root, rel);
  if (!existsSync(dir)) return false;
  if (statSync(dir).isFile()) return readFileSync(dir, "utf8").includes(needle);
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry.startsWith(".")) continue;
    const child = join(rel, entry);
    const full = join(root, child);
    if (statSync(full).isDirectory()) {
      if (sourceContains(root, child, needle, ext)) return true;
    } else if (ext.test(entry) && readFileSync(full, "utf8").includes(needle)) {
      return true;
    }
  }
  return false;
}

/** `[section]` → body text, for a simple TOML manifest. */
function sections(toml) {
  const out = {};
  let current = "";
  for (const line of toml.split("\n")) {
    const header = /^\s*\[([^\]]+)\]\s*$/.exec(line);
    if (header) {
      current = header[1].trim();
      out[current] ??= "";
    } else {
      out[current] = (out[current] ?? "") + line + "\n";
    }
  }
  return out;
}

const DEP_SECTIONS = { dependencies: "normal", "dev-dependencies": "dev", "build-dependencies": "build" };

function rustProjects(root) {
  const cargo = read(root, "Cargo.toml");
  const block = /members\s*=\s*\[([\s\S]*?)\]/.exec(cargo)?.[1] ?? "";
  const members = [...block.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  return members
    .filter((dir) => exists(root, join(dir, "Cargo.toml")))
    .map((dir) => {
      const parsed = sections(read(root, join(dir, "Cargo.toml")));
      const name = /^\s*name\s*=\s*"([^"]+)"/m.exec(parsed.package ?? "")?.[1] ?? dir;
      const deps = [];
      for (const [section, kind] of Object.entries(DEP_SECTIONS)) {
        for (const m of (parsed[section] ?? "").matchAll(/^\s*([\w-]+)\s*=\s*\{[^}]*\bpath\s*=\s*"[^"]+"/gm)) {
          deps.push({ name: m[1], kind });
        }
      }
      return { name, dir, lang: "rust", deps };
    });
}

function tsProjects(root) {
  const dirs = [];
  for (const pattern of ["apps/desktop", "packages/*", "services/*"]) {
    if (pattern.endsWith("/*")) {
      const base = pattern.slice(0, -2);
      if (!exists(root, base)) continue;
      for (const entry of readdirSync(join(root, base)).sort()) {
        if (statSync(join(root, base, entry)).isDirectory()) dirs.push(`${base}/${entry}`);
      }
    } else if (exists(root, pattern)) {
      dirs.push(pattern);
    }
  }
  // A folder git can't track (no package.json, no README) is local noise,
  // not a planned project: skip it so the note is the same on every machine.
  const tracked = dirs.filter(
    (dir) => exists(root, join(dir, "package.json")) || exists(root, join(dir, "README.md")),
  );
  return tracked.map((dir) => {
    if (!exists(root, join(dir, "package.json"))) {
      const readme = exists(root, join(dir, "README.md")) ? read(root, join(dir, "README.md")) : "";
      const summary = readme.split("\n").filter((l) => l.trim() && !l.startsWith("#"))[0] ?? "";
      return { name: dir.split("/").pop(), dir, lang: "planned", deps: [], summary: summary.trim() };
    }
    const pkg = JSON.parse(read(root, join(dir, "package.json")));
    const deps = [];
    for (const [field, kind] of [["dependencies", "normal"], ["devDependencies", "dev"], ["peerDependencies", "normal"]]) {
      for (const dep of Object.keys(pkg[field] ?? {})) {
        if (dep.startsWith("@next-agent/")) deps.push({ name: dep, kind });
      }
    }
    return { name: pkg.name ?? dir, dir, lang: "ts", deps };
  });
}

/**
 * Links that no manifest records. Each is shown only while its probe holds,
 * so the diagram never claims a connection the code no longer has.
 */
const LINKS = [
  {
    from: "@next-agent/desktop", to: "next-agent-desktop", label: "IPC: invoke / Channel", kind: "ipc",
    probe: (r) => sourceContains(r, "apps/desktop/src", "invoke(") && exists(r, "apps/desktop/src-tauri"),
  },
  {
    from: "@next-agent/policy", to: "next-agent-permission-engine", label: "rule parity (check-policy-parity)", kind: "mirror",
    probe: (r) => exists(r, "scripts/check-policy-parity.mjs") && exists(r, "packages/policy"),
  },
  {
    from: "@next-agent/contracts", to: "next-agent-core", label: "event envelope v1", kind: "mirror",
    probe: (r) => exists(r, "packages/contracts") && sourceContains(r, "crates/core/src/events.rs", "fn envelope"),
  },
  {
    from: "@next-agent/vault", to: "next-agent-core", label: "shared vault fixtures", kind: "mirror",
    probe: (r) => sourceContains(r, "packages/vault", "fixtures/vault") && sourceContains(r, "crates/core/src", "fixtures/vault"),
  },
  {
    from: "@next-agent/vault", to: "next-agent-graph", label: "Logseq file-name fixtures", kind: "mirror",
    probe: (r) => sourceContains(r, "packages/vault", "page-file-names") && sourceContains(r, "crates/graph", "page-file-names"),
  },
  {
    from: "next-agent-graph", to: "ext:vault", label: "Logseq graph (pages/, journals/)", kind: "external",
    probe: (r) => exists(r, "crates/graph/src/layout.rs"),
  },
  {
    from: "next-agent-core", to: "ext:9router", label: "HTTP :20128/v1", kind: "external",
    probe: (r) => exists(r, "crates/core/src/router"),
  },
  {
    from: "next-agent-core", to: "ext:git", label: "git CLI (worktrees)", kind: "external",
    probe: (r) => sourceContains(r, "crates/core/src/worktree.rs", 'Command::new("git")'),
  },
  {
    from: "next-agent-core", to: "ext:vault", label: "task records (.md)", kind: "external",
    probe: (r) => exists(r, "crates/core/src/vault.rs"),
  },
];

const EXTERNALS = {
  "ext:9router": "9Router (user-run LLM router)",
  "ext:git": "git",
  "ext:vault": "Markdown vault folder (Logseq graph)",
};

/** Collect the topology of the repo at `root`. */
export function collect(root = ".") {
  const projects = [...rustProjects(root), ...tsProjects(root)];
  const names = new Set(projects.map((p) => p.name));
  const edges = [];
  for (const project of projects) {
    for (const dep of project.deps) {
      if (names.has(dep.name)) edges.push({ from: project.name, to: dep.name, kind: dep.kind, label: "" });
    }
  }
  const externals = new Set();
  for (const link of LINKS) {
    const known = (n) => names.has(n) || n in EXTERNALS;
    if (known(link.from) && known(link.to) && link.probe(root)) {
      edges.push({ from: link.from, to: link.to, kind: link.kind, label: link.label });
      for (const n of [link.from, link.to]) if (n in EXTERNALS) externals.add(n);
    }
  }
  return { projects, edges, externals: [...externals].sort() };
}

const id = (name) => name.replace(/^@next-agent\//, "ts_").replace(/^ext:/, "ext_").replace(/[^A-Za-z0-9_]/g, "_");
const strip = (name) => name.replace(/^next-agent-/, "").replace(/^@next-agent\//, "");

/** Display name; apps have a Rust backend and a TS UI with the same base name. */
function displayName(projects, name) {
  const p = projects.find((x) => x.name === name);
  if (!p) return strip(name);
  if (p.dir.startsWith("apps/")) return p.lang === "rust" ? `${strip(name)} backend (Tauri)` : `${strip(name)} UI`;
  return strip(name);
}

function node(projects, p) {
  return `    ${id(p.name)}["${displayName(projects, p.name)}<br/><small>${p.dir}</small>"]`;
}

const ARROWS = {
  normal: (e) => `${id(e.from)} --> ${id(e.to)}`,
  build: (e) => `${id(e.from)} -->|build| ${id(e.to)}`,
  dev: (e) => `${id(e.from)} -.->|dev| ${id(e.to)}`,
  ipc: (e) => `${id(e.from)} ==>|"${e.label}"| ${id(e.to)}`,
  mirror: (e) => `${id(e.from)} <-.->|"${e.label}"| ${id(e.to)}`,
  external: (e) => `${id(e.from)} -->|"${e.label}"| ${id(e.to)}`,
};

/** Mermaid flowchart source. */
export function renderMermaid(topology) {
  const { projects, edges, externals } = topology;
  const isApp = (p) => p.dir.startsWith("apps/");
  const groups = [
    ["app", "Desktop app", projects.filter(isApp)],
    ["rust", "Rust crates", projects.filter((p) => p.lang === "rust" && !isApp(p))],
    ["ts", "TypeScript packages", projects.filter((p) => p.lang === "ts" && !isApp(p))],
    ["planned", "Planned (no code yet)", projects.filter((p) => p.lang === "planned")],
  ];
  const lines = ["flowchart LR"];
  for (const [key, title, members] of groups) {
    if (members.length === 0) continue;
    lines.push(`  subgraph ${key}["${title}"]`, "    direction TB", ...members.map((m) => node(projects, m)), "  end");
  }
  if (externals.length) {
    lines.push(`  subgraph external["Outside the repo"]`, "    direction TB");
    for (const e of externals) lines.push(`    ${id(e)}[("${EXTERNALS[e]}")]`);
    lines.push("  end");
  }
  for (const edge of edges) lines.push(`  ${ARROWS[edge.kind](edge)}`);
  lines.push(
    "  classDef rust fill:#f6e3d4,stroke:#b7410e,color:#3b1a05",
    "  classDef ts fill:#dbe9fb,stroke:#3178c6,color:#0b2545",
    "  classDef planned fill:#eeeeee,stroke:#999,stroke-dasharray:4 3,color:#555",
    "  classDef ext fill:#e8f5e9,stroke:#2e7d32,color:#1b3d1f",
  );
  for (const p of projects) lines.push(`  class ${id(p.name)} ${p.lang}`);
  for (const e of externals) lines.push(`  class ${id(e)} ext`);
  return lines.join("\n");
}

/** Full Markdown note for the vault. */
export function renderNote(topology) {
  const { projects, edges } = topology;
  const short = (n) => displayName(projects, n);
  const uses = (n) => edges.filter((e) => e.from === n && !e.to.startsWith("ext:") && e.kind !== "mirror").map((e) => short(e.to));
  const usedBy = (n) => edges.filter((e) => e.to === n && e.kind !== "mirror").map((e) => short(e.from));
  const mirrors = (n) =>
    edges.filter((e) => e.kind === "mirror" && (e.from === n || e.to === n)).map((e) => short(e.from === n ? e.to : e.from));
  const kind = { rust: "Rust crate", ts: "TS package", planned: "planned (stub)" };
  const cell = (list) => (list.length ? list.join(", ") : "—");
  const rows = projects.map(
    (p) => `| ${short(p.name)} | ${kind[p.lang]} | \`${p.dir}\` | ${cell(uses(p.name))} | ${cell(usedBy(p.name))} | ${cell(mirrors(p.name))} |`,
  );
  return `---
title: Project Topology
tags: [architecture, generated]
---

# Project Topology

How the separate projects in this repo connect. **Generated** by \`node scripts/gen-topology.mjs\` (\`make topology\`) from the Cargo and package manifests plus a few probed cross-language links; \`make check\` fails if this note is stale. Don't edit by hand.

See [[Architecture Overview]] for what each box does and [[Module Boundaries]] for the rules behind the arrows.

\`\`\`mermaid
${renderMermaid(topology)}
\`\`\`

**Legend:** solid arrow = depends on (manifest) · dotted \`dev\` = test-only dependency · thick arrow = runtime IPC · dotted two-way = kept in sync across languages (parity checks / shared fixtures) · green = outside the repo · grey dashed = planned, README only.

| Project | Kind | Path | Depends on | Used by | Mirrors |
|---|---|---|---|---|---|
${rows.join("\n")}
`;
}
