---
title: Glossary
tags: [reference]
---

# Glossary

- **Workspace** — a local project directory (usually a git repo) the agent works on; its canonical path is the sandbox root.
- **Vault** — a folder of Markdown notes (Obsidian/Logseq-compatible); source of truth for knowledge.
- **Task** — one user request processed end-to-end; owns events, worktree, vault record.
- **Mode** — `plan`, `manual`, `auto`, `bypassPermissions`; see [[Permission Engine]].
- **Tool broker** — Rust component that evaluates and executes every agent tool call.
- **Sandbox** — the policy boundary (workspace/worktree root) enforced by the broker; OS-level from M2.
- **Worktree** — git worktree on branch `next-agent/auto-*` used by auto-mode tasks.
- **Executor** — implementation of `AgentExecutor` (Mock, Builtin, Opencode, Hermes).
- **Router** — 9Router-compatible OpenAI-style endpoint on `:20128`.
- **Audit log** — append-only, hash-chained SQLite table of sensitive decisions.
- **Overlay** — Coucou-style companion window (M3).
- **Channel gateway** — OpenClaw-style daemon for Telegram/Discord/Slack/Gmail (M5).
