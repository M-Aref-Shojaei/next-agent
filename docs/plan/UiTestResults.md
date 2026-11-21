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
| — | — | — | — | — | pending | — |

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
