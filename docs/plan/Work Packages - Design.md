---
title: Work Packages - Design
tags: [plan, tracking, design, desktop]
---

# Design work packages

Use the [Design Workspace Plan](Design%20Workspace%20Plan.md) and [Feature Consolidation Audit](Feature%20Consolidation%20Audit.md) for product decisions and the [Work Package Guide](Work%20Package%20Guide.md) for closing cards. Each card is one checkpoint, typically one focused session. The editor spike and cross-platform checks can span several sessions or operators. Packaging owns the feasibility builds, a desktop UI specialist owns navigation and editing, a storage specialist owns project references and saves, the design and gateway specialist owns tokens and AI, and an independent reviewer owns pilot evidence. Parent dependencies in the tracker apply to all cards.

## DW-01 · Evaluate the editor in the packaged app

### DW-01-A — Pin the candidate and define repeatable checks

- Depends on None.
- Work Record the published OpenPencil SDK, engine, binary and font versions, artifact provenance, and licenses. Select redistributable .fig fixtures for text, layout, components, and variables. Agree on measurement commands, comparable hardware and build settings, and the fidelity and performance tolerances before running the candidate.
- Done when a reviewer can reproduce the baseline and candidate builds and identify the fixtures, expected unsupported features, tolerances, and pass criteria for macOS, Windows, and Linux.

### DW-01-B — Package a minimal offline editor

- Depends on DW-01-A.
- Work In an isolated spike, lazy-load the smallest editor surface in the desktop shell with local CanvasKit, WASM, and fonts. Open a local file, make an edit, undo it, and save through an isolated local boundary.
- Done when the isolated spike produces a packaged build for evaluation or records a reproducible packaging blocker for the decision card. Attach build logs, runtime measurements where available, and steps to reproduce any input, save, or offline loading failure.

### DW-01-C — Measure fidelity and decide whether to integrate

- Depends on DW-01-B.
- Work On supported desktop platforms, run the offline edit, undo, save, and reopen checks, verify keyboard focus and text input, and compare .fig import and export against fixtures. Record installer size, cold start, editor load, and unsupported data.
- Done when the evidence identifies passing and failing platforms and features and the owner records an editor decision before production bundling. If OpenPencil misses the gates, record the current built-in prototype fallback and the smallest remaining evaluation needed to supply safe editable screens before DW-03 can proceed.

## DW-02 · Add the project Design Workspace

### DW-02-A — Define artifact references and project boundaries

- Depends on None.
- Work Trace the existing `/design` direction records, vault-backed `/boards` storage, project discovery, and the proposed CL-03 storage behavior. Define a versioned, project-relative reference contract with stable artifact IDs and revisions, storage for projects without a vault, and explicit resolution of legacy references.
- Done when examples resolve existing boards and direction references without moving files and cover path traversal, missing files, stale revisions, a missing vault, and references from another project. Share the storage decision with CL-03 before implementation.

### DW-02-B — Build the project Design navigation shell

- Depends on DW-02-A.
- Work Add Boards, Screens, Flows, and Design System views to the existing `/design` route using shared UI components. Keep project-scoped selection as users move between views and reset it to the selected project's saved state on project switch.
- Done when focused browser checks cover direct navigation, one highlighted view, keyboard access and focus, empty project state, and restoration of the right artifact after switching projects.

### DW-02-C — Bring existing boards and directions into the workspace

- Depends on DW-02-B, CL-03.
- Work Link existing boards and Design Studio directions into the new views using the agreed reference contract. Reuse the Excalidraw canvas and existing direction controls. Keep saved formats and `/boards` deep links usable.
- Done when an existing project opens its existing board and direction, board export still works, missing files explain how to recover, and a project without a vault can enter Design. A project switch must neither disclose another project's references nor silently redirect writes.

## CL-02 · Consolidate navigation around Design

### CL-02-A — Connect the canonical destinations and retain old routes

- Depends on CL-01, DW-02.
- Work Coordinate a single shell edit with UX-02. Route the activity rail, sidebar, command palette, and project connections to Design or a specific Design view according to their purpose. Name the app's Settings showcase “Component preview” and keep its existing route and the old Boards route reachable.
- Done when each changed route has one current primary destination, and direct `/boards`, `/design`, and `/settings/design-system` links still reach their existing content. Verify board export and the app preview matrix.

### CL-02-B — Verify desktop and narrow-window navigation

- Depends on CL-02-A.
- Work Run direct-link, keyboard, current-state, and responsive checks in Playwright. Follow the project connection to its design system and the Settings path to the app component preview.
- Done when both widths pass and the [UI results](UiTestResults.md) include before-and-after evidence, route compatibility checks, and the owner review decision.

## DW-03 · Edit and preview high-fidelity mockups

### DW-03-A — Save and reopen versioned screens

- Depends on DW-01, DW-02.
- Work Use the agreed editor and reference contract to create a starter document and save screen revisions locally. Check project ownership and bound file reads and writes. Keep last good data available after a failed write and record the current destination before saving.
- Done when fixtures cover initial save, reopen, failed writes, version handling, rejected external paths, and project switching with pending edits. A saved revision used by a flow must remain addressable after a new edit.

### DW-03-B — Import a bounded working copy

- Depends on DW-03-A.
- Work Import a representative .fig file through the approved editor path as a new editable copy. Validate inputs and resource limits, isolate preview rendering, and report unsupported data. Gate export claims on the DW-01 report.
- Done when fixtures show that the original is byte-identical after import and save, supported content survives reopen, and malformed, oversized, or unsupported imports yield useful feedback without a partial saved copy.

### DW-03-C — Connect the editing controls

- Depends on DW-03-A.
- Work Offer the selected engine's controls for frames, text, layout, reused components, variables, and undo in Screens. Keep the manual editor and preview usable offline and provide keyboard access to its controls.
- Done when a supported packaged build can create a mockup, edit and reuse a component, undo an edit, save, and reopen the same design without an account or network connection. Import fidelity remains covered by DW-03-B.

## DW-04 · Author and check clickable flows

### DW-04-A — Validate flow data and add a static map

- Depends on DW-02, DW-03.
- Work Define project-scoped flow records with a start screen, revision-pinned targets, hotspot actions, and expected outcomes. Validate links and reachability against saved revisions and show readable errors on a static map.
- Done when fixture checks distinguish missing starts, stale or cross-project targets, unreachable screens, and valid branches. Invalid flows stay editable and cannot be marked ready for review.

### DW-04-B — Link a board sketch to saved screens and author transitions

- Depends on DW-04-A.
- Work Open a selected Excalidraw board frame as context for a screen without changing the source board. Let the user choose a start, link saved screens, and attach expected outcomes using the supported actions.
- Done when a Playwright scenario opens a frame in Screens, adds and edits transitions, reopens the flow, and finds the expected states with stable source and screen references.

### DW-04-C — Play flows with keyboard and pointer

- Depends on DW-04-A.
- Work Build a read-only prototype player and accessible action list for navigate, back, open overlay, close overlay, and restart. Isolate playback from the development-server preview.
- Done when approved fixtures play both the main and failure branches via keyboard and pointer, preserve predictable overlay focus and restart behavior, and stop at a clear diagnostic when a required revision is missing. Verify the bundled preview makes no network requests.

## DW-05 · Connect project tokens and AI-assisted design

### DW-05-A — Map existing tokens into screen previews

- Depends on DW-02, DW-03.
- Work Read the project's design files with the existing design system path in `design.rs`. Map light and dark tokens and aliases into the editor and screen preview. Report unmapped variables and broken aliases.
- Done when fixture previews show both expected themes, unknown and circular aliases are flagged, and switching projects loads the correct token mapping without changing either project's design files.

### DW-05-B — Draft a screen or revision from a board frame

- Depends on DW-02, DW-03.
- Work Use the existing Design Studio design-role gateway and a selected board frame to supply scoped context for generating or revising a screen. Stage generated content alongside the current revision for review.
- Done when a mock-provider check verifies contextual generation and an unavailable-provider check leaves editing, preview, and existing saves usable. Cancelling the draft leaves the base screen revision intact.

### DW-05-C — Review and approve token changes explicitly

- Depends on DW-05-A, DW-05-B.
- Work Show a proposed token patch separately from the mockup revision. Use the existing design system save boundary after approval, bound to the project and source version used to build the patch.
- Done when reject, accept, and changed-on-disk checks show that only approval writes the intended files. Verify a stale diff stops for a fresh comparison and the approved light/dark changes appear in preview.

## DW-06 · Implement and verify the pilot

### DW-06-A — Record the approved project-opening prototype

- Depends on DW-04, DW-05.
- Work Sketch and play the app's Open Project → discover connections → start a task workflow. Pin the approved screen and flow revisions and capture valid-project, invalid-path, empty-project, missing-vault, and unavailable-model outcomes.
- Done when the owner has reviewed the prototype and its expected outcomes and the handoff identifies the exact board, screen, flow, and project design references.

### DW-06-B — Produce a reviewable implementation diff

- Depends on DW-06-A.
- Work Send the approved artifact references and checks through the existing Design Studio implementation and task review path. Implement the pilot in the task worktree.
- Done when the task produces a focused diff, the review can trace changed behavior to the pinned flow, and the source branch remains available for the owner's decision.

### DW-06-C — Compare the real app with the approved flow

- Depends on DW-06-B.
- Work Run Playwright against the actual running implementation using the approved expected outcomes. Check keyboard traversal and light and dark themes. Attach test evidence and screenshots to [UI Test Results](UiTestResults.md).
- Done when focused and full Playwright, desktop check and build, `pnpm verify`, and the design-token check pass, with Rust workspace checks for native changes. Record any separate live-provider check or blocker and the owner's review decision before integration.
