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
| local-2026-11-24-provider-setup | 2026-11-24 | provider setup branch | 9Router connection setup | Browser tests cover configuration from Settings, endpoint-specific discovery, write-only keys, and exact model IDs through import and save | pending | Owner |
| local-2026-11-22 | 2026-11-22 | uncommitted | Persistent activity rail | Seven keyboard-accessible mode links integrated into app shell; existing routes only | pending | Owner |

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
