---
title: UI Test Results
tags: [plan, testing, ui]
status: live
---

# UI Test Results

Results of the Playwright UI test pipeline. Each run produces a row in the table below. Review the changes, then **approve** or **reject** here. Approved changes are merged; rejected ones are reverted or fixed before the next run.

## Change table (awaiting your review)

| Run | Date | Commit | Change | Diff summary | Status | Reviewer |
|-----|------|--------|--------|--------------|--------|----------|
| local-2026-11-21 | 2026-11-21 | uncommitted | Honest structured diff review + LLM summaries + Playwright CI fixes | Read-only structured hunks; gateway JSON summaries with fallback; Node 22.6, Vite 1420, deterministic smoke tests | approved | User |
| local-2026-11-22-command-palette | 2026-11-22 | uncommitted | Command palette | Cmd/Ctrl+K opens searchable route navigation; no side-effect actions | pending | Owner |
| local-2026-11-24-projects | 2026-11-24 | PR #3 | Project setup and command palette fix | 10 Chromium tests passed, including project opening and creation, vault graph navigation, project switching, and keyboard navigation | pending | Owner |
| local-2026-11-25-navigation | 2026-11-25 | PR #11 | Remove duplicate navigation | 10 Chromium tests passed, including narrow-window Settings access and Boards by name in the palette | merged | Owner |
| local-2026-11-26-project-recovery | 2026-11-26 | PR #11 | Project form recovery (UX-02-D) | 5 focused and 13 full Chromium tests passed. Desktop and narrow cases cover keyboard cancellation, duplicate-folder retry, progress, and preserving focus when a user moves away. Desktop light and 375px dark RTL captures checked. | merged | Owner |
| local-2026-11-26-narrow-shell | 2026-11-26 | `codex/ux-narrow-shell` | Narrow Menu and Chat (UX-02-E) | 14 Chromium tests passed. At 375px, Menu gets its own content row and Escape restores focus. The long timeline scrolls internally and review stays reachable. Production preview captured in dark mode. | pending | Owner |
| local-2026-11-26-chat-draft | 2026-11-26 | `codex/ux-chat-draft` | Chat draft recovery (UX-02-G) | 15 Chromium tests passed. A failed start preserves the exact draft. The pending start blocks duplicates, and edits made during acceptance remain. Synthetic keyboard events cover composition confirmation. | pending | Owner |
| local-2026-11-26-kept-review | 2026-11-26 | `codex/ux-keep-review` | Resume a kept diff review (UX-02-H) | 16 Chromium tests passed. Keyboard task switching, pending tab remount, mock Merge and Discard, and the post-merge notice are covered. | pending | Owner |

The project form tests use the browser's mock API. Native picker interaction still needs packaged-app validation. The project-form render exposed a narrow layout overlap. UX-02-E fixes the shell behavior.

## Planned design workspace reviews

| Change | Review evidence | Status | Reviewer |
|---|---|---|---|
| Design Workspace pilot (DW-02 through DW-06) | Screenshots and Playwright results for storyboard → editable mockup → clickable flow → implementation diff. Include keyboard traversal, light/dark modes, and invalid-path, empty-project, missing-vault, and unavailable-model states. | planned | Owner |

## How to review

1. Open the GitHub Actions run for the PR/commit (linked from the pipeline).
2. Look at the Playwright test report and any screenshots/videos.
3. Edit this table: set **Status** to `approved` or `rejected`, add your name.
4. If approved, the change merges. If rejected, fix and push a new commit.

## Pipeline

The UI pipeline runs on every PR to `main`:

- `.github/workflows/ui.yml` — installs Playwright browsers, builds the app, runs the smoke suite, uploads a HTML report.

Add your result here after each run:

```
| N | YYYY-MM-DD | abc1234 | CodeMirror 6 merge view | Diff reviewer now uses CM6 | ☐ pending | Your Name |
```
