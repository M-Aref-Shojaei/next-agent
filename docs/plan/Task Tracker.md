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
| 5 | Verify authenticated provider chat | Complete the installed app's live 9Router smoke test with an authorized key | awaiting credential setup | Model discovery passed. A test chat returned HTTP 401. A real reply through the installed app is required. | ☐ |
| 6 | CI matrix | macOS/Windows/Linux CI on merged PRs | pending | `.github/workflows/ci.yml` (exists, needs run) | ☐ |
| 9 | Command palette | Cmd/Ctrl+K opens route navigation and Notes retains its search shortcut | shipped in v0.2.0 | Playwright keyboard and focus tests, desktop type checks, and build passed in [PR #3](https://github.com/M-Aref-Shojaei/next-agent/pull/3) | ✓ |
| 10 | Project setup and discovery | Open or create a project and discover its agent instructions, design files, supported vault graph and file tree | shipped in v0.2.0 | Three focused Rust tests and 10 Chromium tests passed. Installed app opened the project and discovered its instructions and design files. | ✓ |
| 11 | Pull request and release | Fix the CI failures and publish verified platform installers from the approved tag | shipped in v0.2.0 | PR and main checks passed. The [release](https://github.com/M-Aref-Shojaei/next-agent/releases/tag/v0.2.0) contains six published installer assets. | ✓ |

## Follow-up work

See the [Product Work Plan](Product%20Work%20Plan.md) for the recommended order and the decisions behind these tasks.

| ID | Task | Depends on | Status | Acceptance evidence | Approved |
|---|---|---|---|---|---|
| REL-01 | Fix the macOS release bundle signature | 11 | planned | A clean download verifies as a signed bundle and installs and launches on an Apple Silicon Mac without local repair. Record signing and notarization requirements for public distribution. | ☐ |
| PROV-01 | Review guided 9Router setup and isolation | — | ready for approval | [PR #10](https://github.com/M-Aref-Shojaei/next-agent/pull/10) passed CI and UI checks. Model import, connection-scoped routing, fallback, and redirect regression tests passed against mock endpoints. | ☐ |
| PS-01 | Load or create project instructions | 10 | planned | Open and New Project read root `AGENTS.md`. New Project creates a starter when absent. Open offers creation if missing. Reopening never overwrites edits, and switching projects reloads instructions from the selected root. | ☐ |
| UX-01 | Audit the desktop UI and UX | CL-01 | planned | Annotated desktop and narrow-window review of project opening, vault and task setup, Design, agent chat, and diff review. Each finding identifies the affected action and a proposed fix. | ☐ |
| UX-02 | Refactor the desktop workspace | UX-01 | planned | Clear primary actions in empty, ready, and error states. Keyboard navigation, project switching, and access to Tasks, Design, review, and Settings pass Playwright for the flows implemented. | ☐ |
| RF-01 | Audit duplicate and redundant code | — | planned | Inventory candidates across desktop UI, Tauri commands, and core services. Each finding names the files and callers, behavioral differences, canonical implementation, and needed regression tests. | ☐ |
| RF-02 | Consolidate verified duplicate code | RF-01 | planned | Focused refactors preserve supported callers, persisted data, and permissions. Focused and full checks pass before redundant paths are removed. | ☐ |
| PT-01 | Add the vault-backed project Tasks tab | CL-03, UX-01 | planned | Quick-add, acceptance checklists, and status changes update the canonical vault Markdown. The Tasks tab opens from the project sidebar by mouse and keyboard and addresses the Tasks findings from UX-01. Two offline projects reopen with independent task lists. | ☐ |
| PT-02 | Link planned tasks to agent runs and review | PT-01 | planned | An explicit Run with Agent action links one or more runs and the diff to the same task. Retries create no duplicate task, and review approval is separate from run success. | ☐ |
| PT-03 | Link work in Notes and Design | PT-01, DW-02, CL-03 | planned | A note TODO or board item can link to the same task. Switching projects cannot update the other project's task or source artifact. | ☐ |
| PT-04 | Add optional task board and Markdown exchange | PT-01, PT-02 | planned | List and Kanban use the same vault task records and status. A sample Markdown import is previewed, deduplicated, and exported with acceptance criteria intact. | ☐ |

## Design Workspace

See [Design Workspace Plan](Design%20Workspace%20Plan.md) for dependencies and acceptance criteria.

| ID | Task | Depends on | Status | Acceptance evidence | Approved |
|---|---|---|---|---|---|
| CL-01 | Remove misleading duplicate navigation | — | ready for approval | 10 Chromium tests passed, including one current rail destination, narrow-window keyboard access to Settings, and Boards from the palette | ☐ |
| CL-02 | Consolidate project and app navigation | CL-01, DW-02 | planned | Project Design is the canonical workspace and the app component preview has a distinct purpose and label, with old links still reachable | ☐ |
| CL-03 | Discover or attach the vault on Open and New Project | 10 | planned | Existing vault documents, links, and graph load as project context without copying or modifying source files. Explicit selection and fresh project-local setup both work. Existing global-vault artifacts stay readable, and two-project tests prove writes stay scoped. | ☐ |
| CL-04 | Consolidate provider setup | PROV-01 | planned | Saved connections and model roles work through one settings entry, with keychain and fallback regressions covered | ☐ |
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
