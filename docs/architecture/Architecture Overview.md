---
title: Architecture Overview
tags: [architecture]
status: draft
---

# Architecture Overview

## Process model
```
┌──────────────────────── Tauri 2 app (single binary) ────────────────────────┐
│  WebView: SvelteKit UI (apps/desktop)                                        │
│     │  invoke() commands          ▲ Channel<AgentEvent> streams              │
│     ▼                             │                                          │
│  Rust core (apps/desktop/src-tauri + crates/*)                               │
│   ├─ core            domain types, task orchestrator, tool broker, SQLite    │
│   ├─ permission-engine  pure policy evaluation (modes, sandbox, rules)       │
│   ├─ process-manager    sidecar lifecycle + health checks                    │
│   ├─ git/worktree       worktree create/diff/merge                           │
│   └─ vault writer       atomic Markdown writes                               │
└──────┬──────────────────────────┬────────────────────────┬───────────────────┘
       │ HTTP (OpenAI-compatible) │ stdio/HTTP JSON-RPC    │ (M5) local IPC
       ▼                          ▼                        ▼
  9Router :20128           agent-gateway (Node)       channel-gateway daemon
  (user-run in M1)         hermes-adapter (uv/Python)  (Telegram, Discord…)
                           — M2+, optional —
```

**Invariant:** sidecars and agents never touch the filesystem or shell directly for task work. They request tools; the Rust **tool broker** evaluates via [[Permission Engine]], executes, and audits. See [[ADR-0004 Permission Engine in Rust Core]].

## Input → Process → Output
| Stage | What happens | Where |
|---|---|---|
| Input | Chat message, CLI invocation, or channel event becomes a `Task` bound to a workspace + vault + mode | UI / CLI / channel-gateway → `core` |
| Validate | Request schema validated; mode resolved (channel tasks capped at `auto`) | `core` |
| Plan/Execute | `AgentExecutor` streams `AgentEvent`s; tool requests go to broker | [[Agent Execution]] |
| Enforce | Path boundary + command classification + rules → `Allow / Ask / Deny` | [[Permission Engine]] |
| Isolate | Auto-mode mutations happen in `next-agent/auto-*` worktree | [[Worktree Isolation]] |
| LLM | Completions through `http://127.0.0.1:20128/v1` | [[Router Integration]] |
| Output | Timeline, tool output, diffs, git status in UI; events + audit in SQLite | [[Desktop UI]], [[Data Model]] |
| Document | Task record written to vault | [[Vault Documentation Service]] |

## Repository layout
```
next-agent/
  apps/desktop/            Tauri 2 app (src-tauri/ Rust, src/ SvelteKit)
  crates/core/             domain models, orchestrator, tool broker, storage
  crates/process-manager/  sidecar lifecycle, health checks
  crates/permission-engine/ policy evaluation (pure, no I/O)
  services/agent-gateway/  TS adapter around opencode-style engines (M2)
  services/router-adapter/ TS helpers / provider config UI glue for 9Router
  services/hermes-adapter/ Python (uv) Hermes boundary (M2, optional)
  services/channel-gateway/ OpenClaw-style connectors (M5)
  packages/contracts/      generated TS types + JSON Schemas from Rust
  packages/ui/             shared Svelte components
  packages/vault/          Markdown parse/index utilities
  docs/                    this vault (architecture/, adr/, upstream/, plan/)
  scripts/                 dev/CI helpers
  Cargo.toml  pnpm-workspace.yaml  README.md
```

Note: Tauri expects `src-tauri` inside the app; the Cargo workspace root includes `apps/desktop/src-tauri` as a member.

## Trust boundaries
1. **WebView ↔ Rust:** Tauri capabilities restrict which commands the UI may call; no `fs`/`shell` plugins exposed to the WebView.
2. **Rust ↔ sidecars:** sidecars are untrusted for side effects; they only receive tool results.
3. **App ↔ LLM:** model output is untrusted input (prompt injection); all tool calls go through the engine regardless.
4. **App ↔ channels (M5):** inbound messages are untrusted; sender allowlists; mode capped.
