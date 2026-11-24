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
| 1 | Honest structured diff review | Read-only old/new hunk renderer; whole-task Merge/Discard/Keep remain authoritative | approved | `apps/desktop/tests/diff-merge.test.ts`, Playwright smoke suite | ✓ |
| 2 | LLM task summaries | Gateway-backed JSON summarizer with deterministic fallback when router unavailable | approved | Rust compile checks; existing `VaultWriter` fallback tests; adapter tests pending | ✓ |
| 3 | Playwright smoke test | Deterministic browser smoke suite against Vite on port 1420; CI build validation | approved | Playwright: 5 passed | ✓ |
| 4 | Security hardening | Canonical execution paths, worktree identity/stale-review validation, and regression tests from security audit | ready for approval | Workspace Rust tests: 446 core unit tests passed; permission corpus passed | ☐ |
| 5 | Real-router run | Test with a live API key and real provider chat | pending | Router health e2e | ☐ |
| 6 | CI matrix | macOS/Windows/Linux CI on merged PRs | pending | `.github/workflows/ci.yml` (exists, needs run) | ☐ |
| 9 | Command palette | Cmd/Ctrl+K opens route navigation and Notes retains its search shortcut | ready for approval | Playwright keyboard and focus tests passed; desktop type check and build passed | ☐ |
| 10 | Project setup and discovery | Open an existing folder or create a new project, then show its agent instructions, design files, vault graph and file tree | ready for approval | 3 focused Rust tests and 10 Chromium tests passed; `pnpm verify`, Clippy and desktop frontend build passed | ☐ |
| 11 | Pull request and release | Fix actionable PR failures, verify required checks and publish through the approved release workflow | awaiting CI and owner approval | Release dry run verified tag/version matching and missing-installer handling; hosted release matrix pending | ☐ |

## Definition of done (M1)

- All steps accepted on macOS, Windows, Linux.
- Security review of permission engine & path checker done.
- Docs in this vault updated; ADRs accepted.
- UI test pipeline green on every PR merge.

## Change log (UI pipeline results)

See [UiTestResults](UiTestResults.md) for the table of UI changes awaiting your approval.
