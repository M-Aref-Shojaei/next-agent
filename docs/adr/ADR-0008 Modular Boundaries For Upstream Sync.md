---
title: ADR-0008 Modular Boundaries For Upstream Sync
tags: [adr, architecture]
status: proposed
---

# ADR-0008 Modular Boundaries For Upstream Sync

- **Status:** proposed · **Date:** 2026-11-11

## Context
Every external part of this project moves on someone else's schedule: 9Router, opencode,
Hermes, SvelteKit, Tauri, SQLite bindings, even the event envelope. If we couple to their
internals, an upstream release becomes a rewrite. We need parts that can be replaced or
re-vendored without touching the rest of the system.

## Decision
Every cross-boundary dependency sits behind a **seam**: a small trait or versioned
contract owned by us, with adapters on the outside.

| Seam | Contract owned by us | Swappable part |
|---|---|---|
| Agent runtime | `AgentExecutor` trait | built-in loop, opencode, Hermes, mock |
| Enforcement | `ToolBroker` trait | real broker, scripted test broker |
| LLM transport | `CompletionClient` trait | 9Router, any OpenAI-compatible endpoint |
| Wire format | `packages/contracts` + `EventEnvelope.v` | Rust ↔ TS ↔ sidecars |
| Filesystem sandbox | `PathResolver` trait | lexical (tests), real canonicalizing |
| Git isolation | `WorktreeProvider` trait | git CLI, libgit2, none |
| Task records | `TaskRecorder` trait | vault Markdown, JSON export |
| Sidecars | versioned JSON-RPC over stdio | `services/*` implementations |

Rules:
1. A module depends on another module's **contract**, never its internals. Crates do not
   reach into each other's private modules; the public surface is `lib.rs`.
2. Every seam has a **conformance suite** that any implementation must pass; adding an
   implementation means adding a test run, not editing callers.
3. Upstream versions are pinned in `upstream.lock.json` with the reason and the adapter
   that owns them. Bumping a pin touches only that adapter.
4. Breaking a contract bumps its version (`EventEnvelope.v`, `MIGRATION_Vn`) and ships a
   migration or a compatibility shim — never a silent change.
5. No module imports a transport directly (no `reqwest` outside the LLM adapter, no
   `rusqlite` outside `store.rs`, no `std::fs` in the permission engine).

## Consequences
- (+) Upstream updates are local edits plus a test run.
- (+) Tests can substitute fakes for anything, which keeps the suite fast and offline.
- (−) More traits and one extra indirection per seam.
- (−) Requires discipline: a shortcut call across a boundary is the failure mode to review for.
