---
title: Roadmap
tags: [plan]
status: draft
---

# Roadmap

The goal is one cohesive Tauri 2 app that is **local-first, transparent, auditable, cross-platform** (macOS, Windows, Linux from M1).

The [Product Work Plan](Product%20Work%20Plan.md) records the current release, review queue, and implementation order for simplifying the desktop app, integrating the design workflow, and adding a project backlog. The [Task Tracker](Task%20Tracker.md) holds the corresponding work items.

## Guiding principles
1. **Every side effect goes through the Rust core.** Agents propose tool calls; the core's tool broker evaluates them with the [[Permission Engine]] and executes them. No component bypasses it.
2. **Markdown is the source of truth** for knowledge; SQLite holds metadata, events and audit only.
3. **Auditable by default.** Every decision is recorded; bypass actions go to a tamper-evident log.
4. **Upstreams behind interfaces.** opencode, Hermes, OpenClaw, etc. are adapters, never the core.
5. **Thin vertical slices.** Each milestone ships something usable end-to-end.

## Milestones

| # | Name | Outcome | Depends on |
|---|------|---------|------------|
| M0 | Foundations | Monorepo scaffold, toolchains, CI on 3 OSes, contracts pipeline | — |
| M1 | Vertical slice | Chat → agent → permission-checked tools → worktree diff → merge → vault record | M0 |
| M2 | Hardening & real engines | OS-level sandboxing, opencode adapter, Hermes adapter (optional), headless CLI | M1 |
| M3 | Companion overlay | Coucou-style always-on-top status/approval overlay | M1 |
| M4 | Knowledge vault UX | Outliner/editor, backlinks, graph, search, journals | M1 |
| M5 | Channel gateway | OpenClaw-style daemon: Telegram, Discord, Slack, Gmail | M2 |

### M0 — Foundations (~1 week)
- Repo layout per [[Architecture Overview]]; `pnpm-workspace.yaml` + Cargo workspace.
- Tauri 2 app boots on macOS/Windows/Linux in CI (GitHub Actions matrix).
- Lint/format/test: `cargo fmt`, `cargo clippy`, `cargo test`, `pnpm lint`, `pnpm check`, `pnpm test`.
- Contracts codegen Rust → TS chosen and wired ([[Event Contracts]]).
- Upstream licence & architecture review completed ([[Upstream Evaluation]]).
- **Exit:** empty app window builds and runs on all three OSes from CI artifacts.

### M1 — Vertical slice (~4–6 weeks)
See [[M1 Vertical Slice]] for the step list. Seven deliverables:
1. Workspace setup & SQLite metadata.
2. `AgentExecutor` abstraction with event streaming.
3. Permission engine: `plan`, `manual`, `auto` (sandboxed), `bypassPermissions` (unlocked + audited).
4. Git worktree isolation for auto-mode edits.
5. Automated Markdown task documentation.
6. 9Router endpoint config + health check (`http://127.0.0.1:20128/v1`).
7. Workstation UI: mode badge, chat timeline, diff reviewer, settings.

**Exit:** a user opens a repo, asks for a change in `auto` mode, watches the timeline, reviews a side-by-side diff, merges, and finds a task record in the vault — with every decision in the audit log.

### M2 — Hardening & real engines
- OS-level sandbox for spawned processes (macOS Seatbelt, Linux Landlock/bubblewrap, Windows restricted token/AppContainer). Closes the gap in [[ADR-0004 Permission Engine in Rust Core]].
- opencode adapter behind `AgentExecutor` (via `services/agent-gateway`).
- Hermes adapter (`services/hermes-adapter`, `uv`-managed) — optional.
- Headless CLI (`next-agent run --mode auto`, `--dangerously-skip-permissions`) for cron/server use.
- Signed/notarized builds, auto-update.

### M3 — Companion overlay
- Second Tauri window: transparent, always-on-top; notch/Dynamic-Island style on macOS, corner widget elsewhere.
- Shows active task, mode, pending approvals; approve/deny from the overlay.

### M4 — Knowledge vault UX
- Markdown editor + outliner blocks (Logseq concepts), backlinks, FTS5 search, daily journals, graph view.

### M5 — Channel gateway
- `services/channel-gateway` daemon; inbound events become tasks in the same pipeline.
- Channel-originated tasks can never enter `bypassPermissions`; approvals route back to desktop/overlay.

## Cross-cutting tracks
- **Security:** threat model per milestone; fuzz the command classifier; path-escape test corpus.
- **Docs:** completed work is dogfooded into this vault.
- **Accessibility & i18n:** keyboard-first, screen-reader labels, RTL-safe layout.
