---
title: Development Setup
tags: [guide]
status: draft
---

# Development Setup

## Prerequisites
| Tool | Version | Notes |
|---|---|---|
| Rust | stable (via `rustup`) | `rustup component add clippy rustfmt` |
| Node.js | 22 LTS+ | |
| pnpm | 10+ | `corepack enable` |
| uv | latest | only for `services/hermes-adapter` (M2) |
| git | 2.40+ | worktree features |
| Tauri OS deps | — | macOS: Xcode CLT · Windows: MSVC Build Tools + WebView2 · Linux: `libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev` |
| 9Router | — | running on `127.0.0.1:20128` for real LLM calls (MockExecutor works without) |

Current dev machine (2026-11-10): Node 24, uv, and git present; **Rust and pnpm still to install** — the machine had no outbound DNS, so both installs failed.

## Tests without dependencies
Until pnpm/Rust are available, the TypeScript suite runs on Node 24 alone:
```sh
node scripts/test.mjs        # every *.test.ts under packages/, services/, apps/
node scripts/check-workspace.mjs
```
Node 24 executes `.ts` directly (type stripping) and ships `node:sqlite`, which is why no build step is needed.

## Commands (once scaffolded)
```sh
pnpm install
pnpm tauri dev          # run desktop app
cargo test --workspace  # Rust tests
pnpm -r test            # TS tests
pnpm -r check           # svelte-check / tsc
```
