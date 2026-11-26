---
title: Work Packages - UX and Refactoring
tags: [plan, tracking, desktop]
---

# UX and refactoring work packages

The product reviewer owns screenshots and findings. Desktop specialists own UI slices. A Rust specialist owns any confirmed core refactor. Each card targets one reviewable checkpoint. The application-wide audit and each implementation slice may take one to two focused sessions. If the evidence points to several unrelated changes, register a card for each before editing.

## CL-01 · Review the navigation cleanup

### CL-01-A — Close the duplicate navigation review

- Depends on None.
- Work Review the existing [PR #11](https://github.com/M-Aref-Shojaei/next-agent/pull/11) changes in the activity rail and command palette. Compare the current route highlight and keyboard access to Chat, Boards, and Settings with the acceptance row.
- Done when the reviewer records results from `pnpm --filter @next-agent/desktop test:ui` and the current PR checks. Record the owner's approval and the integrated commit before closing the package. Keep review open while approval is pending.

## UX-01 · Audit the current desktop

### UX-01-A — Audit Open and New Project

- Depends on CL-01.
- Work Walk the first launch and project picker in `ProjectActions.svelte` and `ProjectConnections.svelte` using a blank folder and an existing project. Check missing instructions, an unavailable vault, switching projects, and returning to recent projects. Cover the planned Tasks entry with a walkthrough of the proposed flow.
- Done when the audit has annotated desktop and narrow-window evidence, reproduction steps, severity, the desired result, and a proposed fix per issue. Treat planned screens as walkthroughs and identify the current build used.

### UX-01-B — Audit navigation, Design, chat, and review

- Depends on CL-01.
- Work Use `ActivityRail.svelte`, `Sidebar.svelte`, the command palette, and the existing Design and Chat routes. Follow a developer from project context to a board, an agent run, and its diff. Repeat by keyboard and in a narrow window.
- Done when findings show which action is hard to find or finish, with screenshots or keyboard steps including focus, route highlight, and the path back to the project. Record which findings are already handled by CL-01 or CL-02.

### UX-01-C — Turn findings into a prioritized change list

- Depends on UX-01-A, UX-01-B.
- Work Combine the findings into a list ordered by blocked actions, errors, and everyday friction. Attach each issue to UX-02, PT-01, CL-02, or the appropriate feature card. Record an explicit reason for any issue deferred.
- Done when each high-priority finding has one owner, one change location, and one observable acceptance check. Keep the acceptance criteria in the affected cards and UI review evidence in the existing results page.

## UX-02 · Fix the audited flows

### UX-02-A — Simplify project opening and empty states

- Depends on PS-01, CL-03.
- Work Apply the UX-01 findings to the project picker and connection panel. Put the next action where a first-time user can find it. Cover loading, missing inputs, a cancelled picker, and recovery from a failed open.
- Done when `projects.spec.ts` verifies the expected first action, focus after cancelling, and recovery on a retry. Attach screenshots for an empty project and a project with connections.

### UX-02-B — Simplify chat and diff review

- Depends on UX-01.
- Work Apply UX-01 to the existing Chat route and review controls. Keep permission prompts and explicit Merge, Discard, and Keep decisions available. Use existing tokens and shared controls.
- Done when `smoke.spec.ts` and the focused diff tests cover keyboard access and the expected approval, error, and completed states. Record a before-and-after walkthrough for the diff review.

### UX-02-C — Verify the desktop workflow at both widths

- Depends on UX-02-A, UX-02-B.
- Work Test the changed flows at normal and narrow widths, with a keyboard pass through project switching, Design, review, and Settings. Capture evidence for the routes implemented in this PR. PT-01 verifies its own Tasks interactions.
- Done when `pnpm --filter @next-agent/desktop check` and `pnpm --filter @next-agent/desktop test:ui` pass, and the tracker links an entry in [UI Test Results](UiTestResults.md) with screenshots and review status.

### UX-02-D — Improve project form recovery with existing services

- Depends on None.
- Work Simplify Open and New Project in the current sidebar. Use one form at a time, report progress and errors next to the fields, and preserve attempted values. Restore focus after picker fallback, cancel, or success when the user is still in the flow.
- Done when desktop and narrow Chromium tests cover keyboard access, cancellation, duplicate-name retry, and a deferred registration that preserves focus outside the form. Record local render evidence and the owner review row. PS-01, CL-03, and the broader UX-02-A work retain their existing scope.

### UX-02-E — Separate the narrow menu from Chat

- Depends on CL-01.
- Work Give the expanded narrow Menu the content row. Restore Chat on close or route navigation and keep the composer and review panel in flow in short windows.
- Done when Chromium verifies keyboard access, focus on dismissal, and non-overlapping composer and review controls in a 375px short viewport. Desktop layout and existing project forms must keep working.

### UX-02-F — Verify the native folder picker

- Depends on UX-02-D.
- Work Build the merged code for macOS. In a temporary project, select and cancel the Open Project folder picker and the New Project parent folder picker.
- Done when the record names the tested build and confirms the manual path fallback, captured paths, successful registration, and focus restoration. Keep the task open if native GUI interaction is unavailable.

### UX-02-G — Keep the Chat draft when a task cannot start

- Depends on None.
- Work Keep the draft while Chat waits for registration. Clear it only after the task is accepted and the draft still matches the submitted text.
- Done when tests cover rejection, an exact multiline retry, successful creation, and edits made while a start is pending.

### UX-02-H — Resume a kept diff review

- Depends on None.
- Work Render Keep as a pending review decision. Keep Merge and Discard available when the user reopens a kept task. Keep terminal decisions and notices visible after merge or discard.
- Done when the browser fixture uses Keep, switches tasks, returns, and finishes the review. Verify both terminal actions using mocked outcomes.

## RF-01 · Prove which code is redundant

### RF-01-A — Inventory overlapping desktop paths

- Depends on None.
- Work Trace callers from the Svelte routes and components for navigation, project setup, vault access, Design, and provider settings. Start from the [Feature Consolidation Audit](Feature%20Consolidation%20Audit.md). Record existing features that already own a replacement.
- Done when the inventory lists both implementations, routes and callers, behaviors at risk, a candidate to keep, and a test for each overlap. Record the evidence for retaining any look-alike paths that serve different callers.

### RF-01-B — Inventory overlapping Tauri commands and core paths

- Depends on None.
- Work Trace desktop bridge calls to registered Rust commands, then follow the backing project, vault, provider, and review services. Check existing tests, settings formats, permission checks, and failure behavior.
- Done when each candidate has file and caller references, compatibility and rollback notes, and a regression test to exercise both call paths. Flag unknown consumers for investigation before proposing removal.

### RF-01-C — Pick the first safe refactor

- Depends on RF-01-A, RF-01-B.
- Work Review both inventories and pick the smallest verified overlap with a clear owner. Record the surviving path, callers to move, behavior to preserve, and the boundary between migration and deletion.
- Done when the independent reviewer can reproduce the overlap and the tracker records the selected files, baseline checks, and rollback procedure for RF-02.

## RF-02 · Remove proven duplicate paths

### RF-02-A — Lock down the existing behavior

- Depends on RF-01.
- Work For the first candidate from RF-01, exercise both paths against supported inputs, existing data, and expected failures. Add targeted tests where coverage is missing.
- Done when the baseline passes on the unchanged implementation and the card links each test to the caller or data format it protects.

### RF-02-B — Move callers to the surviving implementation

- Depends on RF-02-A.
- Work Switch the selected callers to the existing retained implementation. Keep compatibility entry points while callers move and record any necessary persisted-data handling.
- Done when the focused regression tests and `pnpm verify` pass, a caller search finds no supported callers on the redundant path, and the reviewer verifies state and permission behavior.

### RF-02-C — Remove the redundant implementation and recheck

- Depends on RF-02-B.
- Work Remove only the now-unused implementation and update references. Split any additional candidate into its own tracker row and reviewable change.
- Done when focused tests, shared checks, and the changed surface's build or test suite pass again. Record the deletion, test evidence, and rollback point.
