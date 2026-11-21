---
title: Task Tracker
tags: [plan, tracking]
status: live
---

# Task Tracker

All remaining work for the Next Agent project. Each task has a status, the change it makes, and an approval column. UI changes are verified through the Playwright pipeline (`docs/plan/UiTestResults.md`) before merging.

## Remaining M1 tasks

| # | Task | Change | Status | Tests | Approved |
|---|------|--------|--------|-------|----------|
| 1 | CodeMirror 6 merge view | Replace plain HTML table diff in `DiffViewer.svelte` with a CM6 merge editor (old/new + per-hunk accept/reject) | pending | `apps/desktop/tests/diff-merge.test.ts`, Playwright diff review | ☐ |
| 2 | LLM task summaries | Wire an LLM summarizer for vault task records; deterministic fallback when router unavailable | pending | `packages/vault` golden-file tests | ☐ |
| 3 | Playwright smoke test | End-to-end UI smoke: auto → worktree → review → vault record → merge | pending | Playwright suite in `.github/workflows/ui.yml` | ☐ |
| 4 | Security review | Permission engine & path checker security audit | pending | 53 engine tests + path-escape corpus | ☐ |
| 5 | Real-router run | Test with a live API key and real provider chat | pending | Router health e2e | ☐ |
| 6 | CI matrix | macOS/Windows/Linux CI on merged PRs | pending | `.github/workflows/ci.yml` (exists, needs run) | ☐ |

## Definition of done (M1)

- All steps accepted on macOS, Windows, Linux.
- Security review of permission engine & path checker done.
- Docs in this vault updated; ADRs accepted.
- UI test pipeline green on every PR merge.

## Change log (UI pipeline results)

See [UiTestResults](UiTestResults.md) for the table of UI changes awaiting your approval.
