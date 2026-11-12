---
title: ADR-0005 Git Worktrees for Auto Mode
tags: [adr, git]
status: proposed
---

# ADR-0005 Git Worktrees for Auto Mode

- **Status:** proposed · **Date:** 2026-11-10

## Decision
Auto-mode mutations happen in `git worktree`s on branches `next-agent/auto-<id>` under `<ws>/.next-agent/worktrees/`, excluded via `.git/info/exclude`. Merge only after user review. Details: [[Worktree Isolation]].

## Consequences
- (+) Main tree untouched; easy discard; parallel tasks possible.
- (−) Requires git; dependency dirs (`node_modules`, `target`) are per-worktree → slower first build. Consider shared caches (`CARGO_TARGET_DIR`, pnpm store) later.

## Alternatives
Copy-on-write directory snapshots (not portable), stash-based (fragile), shadow git repo (possible fallback for non-git workspaces).
