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

## Design Workspace

See [Design Workspace Plan](Design%20Workspace%20Plan.md) for dependencies and acceptance criteria.

| ID | Task | Depends on | Status | Acceptance evidence | Approved |
|---|---|---|---|---|---|
| CL-01 | Remove misleading duplicate navigation | — | ready for approval | 10 Chromium tests passed, including one current rail destination, narrow-window keyboard access to Settings, and Boards from the palette | ☐ |
| CL-02 | Consolidate project and app navigation | CL-01, DW-02 | planned | Project Design is the canonical workspace and the app component preview has a distinct purpose and label, with old links still reachable | ☐ |
| CL-03 | Unify project artifact storage and vault setup | DW-02 | planned | Existing notes and boards remain readable, fresh projects can save locally, and two-project switching tests prove writes stay scoped | ☐ |
| CL-04 | Consolidate provider setup | Provider connection migration and review | planned | Saved connections and model roles work through one settings entry, with keychain and fallback regressions covered | ☐ |
| DW-01 | Evaluate OpenPencil in the packaged desktop app | — | planned | Offline edit/save/reopen on supported desktop platforms, a representative .fig compatibility report, and comparable cold-start and bundle measurements | ☐ |
| DW-02 | Add a project-scoped Design Workspace | — | planned | One Design destination links existing boards and prototypes to the active project, with accessible navigation and no migration of existing files | ☐ |
| DW-03 | Edit and preview high-fidelity mockups | DW-01, DW-02 | planned | Open an imported copy or create a mockup, edit reusable components, undo, save and reopen without changing the original | ☐ |
| DW-04 | Author and check clickable flows | DW-02, DW-03 | planned | Board sketch opens an editable mockup, a keyboard-operable flow runner handles expected and failure paths, and invalid links are flagged | ☐ |
| DW-05 | Connect project tokens and AI-assisted design | DW-02, DW-03 | planned | Light/dark token mappings apply to previews, AI changes are reviewable, and existing tokens are updated only through an explicit approved diff | ☐ |
| DW-06 | Implement and verify the pilot workflow | DW-04, DW-05 | planned | Approved prototype becomes a worktree diff, with matching Playwright checks against the running app and owner review evidence | ☐ |

## Definition of done (M1)

- All steps accepted on macOS, Windows, Linux.
- Security review of permission engine & path checker done.
- Docs in this vault updated; ADRs accepted.
- UI test pipeline green on every PR merge.

## Change log (UI pipeline results)

See [UiTestResults](UiTestResults.md) for the table of UI changes awaiting your approval.
