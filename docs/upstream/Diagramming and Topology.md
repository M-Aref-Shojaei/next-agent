---
title: Diagramming and Topology
tags: [upstream, architecture, diagrams]
status: draft
---

# Diagramming and Topology

Goal: see how Next Agent fits together as box diagrams and a dependency map. That means crates, packages and services, the seams in [[Module Boundaries]], and the flow from chat to the vault record. The diagrams must stay true as the code changes. Later, the same kind of view could ship inside the app's vault (M4).

Researched 2026-11-13. Licences come from project pages and package listings; confirm each one against the repo's LICENSE before adopting it.

## Decision (2026-11-13)
Mermaid is the diagram format. The project-to-project dependency map is generated as Mermaid in [[Project Topology]] (`make topology`; `make check` fails when it is stale). LikeC4, D2 and the external graph tools are not adopted for now; in-app Mermaid rendering is planned for M4.

## Recommendation (original research)

Use two layers that check each other:

1. **Intended architecture, written as code:** **LikeC4**. A small model in the repo describes the containers, components and seams. LikeC4 renders interactive C4-style box diagrams from it, in VS Code, a live-reload dev server or a static site. This is the "box diagram" view.
2. **Actual dependencies, read from the code:** **cargo-depgraph** for the Rust crates and **dependency-cruiser** for the TypeScript packages. Both draw the real dependency graph. dependency-cruiser can also enforce rules, which complements `scripts/check-boundaries.mjs`. This shows drift between the intended model and the actual code.

For a one-off overview of an unfamiliar repo, **GitDiagram** generates a Mermaid diagram with an LLM. It is good for exploring, but it doesn't stay in sync.

## Candidates

| Repo | What it gives | Licence | Fit for us |
|---|---|---|---|
| [likec4/likec4](https://github.com/likec4/likec4) | Architecture-as-code DSL (C4-inspired) → live, navigable diagrams; VS Code extension, dev server, static export, CLI for CI | MIT | **Primary.** The model lives in `docs/architecture/`, sits beside the notes and is reviewed in PRs |
| [jplatte/cargo-depgraph](https://lib.rs/crates/cargo-depgraph) | Graphviz graph of Cargo workspace dependencies from `cargo metadata` | *verify* | **Crate topology.** `core → permission-engine`, `desktop → core`, etc. |
| cargo-arc / cargo-modules / dep_graph_rs | Module-level `use` graphs inside a crate (arc diagram / tree / DOT) | *verify* | Optional, for looking inside `crates/core` |
| [sverweij/dependency-cruiser](https://github.com/sverweij/dependency-cruiser) | TS/JS dependency graphs and rule validation | MIT | **Package topology** for `apps/desktop`, `packages/*` |
| [ahmedkhaleel2004/gitdiagram](https://github.com/ahmedkhaleel2004/gitdiagram) | LLM turns a repo into an interactive Mermaid diagram with clickable nodes | MIT | Quick exploration only; sends code to an LLM (self-host or use our router) |
| Archify (agent skill) | Coding agent analyses the code → typed JSON → HTML architecture / sequence / data-flow diagrams | *verify* | Interesting for agent-generated docs; popularity figures in blogs vary, so verify first |
| [mermaid-js/mermaid](https://github.com/mermaid-js/mermaid) | Text diagrams rendered in Markdown | MIT — *verify* | **In-app (M4):** render ```` ```mermaid ```` blocks in vault notes and task records |
| [xyflow/xyflow](https://github.com/xyflow/xyflow) (Svelte Flow) | Interactive node graph UI for Svelte | MIT (Pro examples excluded) | **In-app (M4):** an interactive topology or graph view in our SvelteKit UI |
| [terrastruct/d2](https://github.com/terrastruct/d2) | Text → diagram language; TALA layout engine now open source | MPL-2.0 | Good output, but it's a Go binary, so it would need a sidecar (see [[ADR-0007 Minimize Sidecars in M1]]). Reference only |
| [excalidraw/excalidraw](https://github.com/excalidraw/excalidraw) | Hand-drawn whiteboard | MIT — *verify* | React-based, so it's awkward inside Svelte; maybe for freehand sketches later |
| [tldraw/tldraw](https://tldraw.dev/legal/tldraw-license) | Infinite-canvas SDK | Proprietary tldraw licence (watermark; paid key for commercial production) | **Avoid** in the shipped app |

## Proposed next steps
- [ ] Add `docs/architecture/next-agent.c4`, a LikeC4 model of the workspace: desktop app, core (broker, runner, router, worktree, vault, store), permission engine, process manager, services. Include the M1 flow as a dynamic view.
- [ ] Add a `make diagrams` target: `cargo depgraph --workspace-only | dot -Tsvg` and `depcruise --output-type dot apps packages | dot -Tsvg` into `docs/architecture/generated/`.
- [ ] Optional: let dependency-cruiser rules take over the TS half of `check-boundaries.mjs`.
- [ ] M4: render Mermaid in vault notes, and add a Svelte Flow topology view fed by the same data.

## Sources
- LikeC4 README and licence: https://github.com/likec4/likec4
- D2 / TALA open-sourced: https://d2lang.com/blog/tala-is-open-source/
- tldraw licence: https://tldraw.dev/legal/tldraw-license
- cargo-depgraph: https://lib.rs/crates/cargo-depgraph · cargo-arc: https://docs.rs/crate/cargo-arc/0.2.2
- dependency-cruiser: https://github.com/sverweij/dependency-cruiser
- Svelte Flow / xyflow licence: https://github.com/xyflow/xyflow
- GitDiagram: https://github.com/ahmedkhaleel2004/gitdiagram
