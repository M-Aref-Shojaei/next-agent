---
title: Work Packages - Project and Tasks
tags: [plan, tracking, desktop, vault, tasks]
---

# Project and tasks work packages

Use the parent outcomes and approvals in the [Task Tracker](Task%20Tracker.md). Each card is one reviewable checkpoint, sized for one focused session. The implementation role is in the card. Use separate owners for independent backend and UI work, with one integrator for the shared project store, Tauri command registration, and navigation. Record actual commands and results in the execution table.

## PS-01 · Project instructions

### PS-01-A — Read and scaffold root instructions

- Depends on 10.
- Work Rust specialist. Extend the project service in `projects.rs` with bounded reads of root `AGENTS.md` and a create-if-absent starter shared by New Project and the explicit starter action. Reuse registered workspace identity and keep Open discovery read-only.
- Done when Rust fixtures verify existing content stays byte-for-byte intact, a new project has a starter, a repeat or competing create preserves the existing file, and unreadable files or symlinks produce a recoverable result without touching a different path.

### PS-01-B — Load instructions for the selected project

- Depends on PS-01-A.
- Work Desktop specialist. Use `app.svelte.ts`, `ProjectActions.svelte`, and `ProjectConnections.svelte` to show loaded instructions or offer starter creation on Open. Carry the initiating project ID through the request and guard against late responses after a switch.
- Done when `projects.spec.ts` verifies New Project, Open with a missing file, explicit creation, and a rapid switch between two projects with different content. Verify by keyboard and record the UI result with screenshots.

### PS-01-C — Use project instructions during agent runs

- Depends on PS-01-A.
- Work Rust specialist. Load the selected root's instructions into the run context at the existing `tasks.rs` plan boundary, including when an Auto run executes in a worktree. Apply existing permissions and approval checks to every tool call.
- Done when the mock provider captures different instructions for consecutive runs in two roots, run metadata retains the original user prompt, and a fixture with an instruction requesting a disallowed tool still follows the normal permission decision.

## CL-03 · Project vault attachment

### CL-03-A — Persist the project vault destination

- Depends on 10.
- Work Rust and data specialist. Use canonical workspace and vault identities and the existing `vaults` and `workspace_vaults` schema in `store.rs`. Define a preferred attachment and destination per project, and document how legacy global vault files remain reachable. Prefer an existing saved attachment. When Open or New Project discovers one supported project-local vault, load it in place. Offer a choice for ambiguous results and require explicit selection for an external vault.
- Done when Rust fixtures show an explicitly attached existing vault survives a DB reopen, invalid attachment preserves the prior choice, another project's attachment is unaffected, and the saved global vault setting remains readable.

### CL-03-B — Open existing vaults and initialize fresh ones

- Depends on CL-03-A.
- Work Rust and graph specialist. Separate opening from initialization at the `notes.rs` and `crates/graph` boundary, where the current open path calls `Graph::init`. Open existing documents, backlinks, and graph in place. Use an explicit setup action to initialize a supported project-local vault. Validate the attachment on reads and bind writes to the initiating project and vault identity.
- Done when a fixture with an incomplete but supported graph is readable without adding or changing source files, explicit initialization produces a reopenable graph, and a delayed edit after a project switch either completes in its initiating vault or fails with its state intact.

### CL-03-C — Route project artifacts through the attachment

- Depends on CL-03-A.
- Work Rust specialist. Change project-scoped board writes in `commands.rs`, Design board reads in `design.rs`, and agent run records in `tasks.rs` to use the originating workspace and its vault destination. Keep user-wide Settings and learning callers on their existing global path. Preserve legacy global artifacts through a deliberate read path.
- Done when tests with two projects and a legacy vault prove old files remain readable, concurrent board saves retain their original destination, and completing or reviewing a run after a switch cannot create or rewrite its record under the other project.

### CL-03-D — Offer vault setup from project context

- Depends on CL-03-B, CL-03-C.
- Work Desktop specialist. Put discovery, explicit folder selection, and project-local initialization in project setup. Display the destination before saving artifacts. Rebind Notes, Boards, and Design to the selected project, preserving a reachable path for legacy artifacts.
- Done when the Playwright project flow covers automatic loading of a single discovered project-local graph, a choice for ambiguous results, explicit attachment, fresh local setup, cancelled selection, and switching projects. Record the native two-project write fixtures and the UI run in the tracker and UI results.

## PT-01 · Offline project task list

### PT-01-A — Store planned tasks in vault Markdown

- Depends on CL-03, UX-01.
- Work Rust and data specialist. Define a versioned Markdown record with a stable task ID, title, status, and acceptance checklist. Scope storage to the attached project destination. Reuse existing atomic vault writing conventions and the app's Markdown model where applicable. Keep the SQLite execution rows and generated run records separate.
- Done when file fixtures round-trip all fields, distinguish two projects sharing a vault, preserve a newer edit when a write is stale, and leave existing Notes pages and run records untouched.

### PT-01-B — Add the project Tasks route and list

- Depends on PT-01-A.
- Work Desktop specialist. Add a sidebar entry, quick-add, task details with editable acceptance checklist, and To Do, In Progress, Review, and Done status controls. Apply the relevant UX-01 findings and reuse the existing desktop components and tokens.
- Done when a focused Playwright spec opens Tasks by mouse and keyboard, quick-adds without a provider, checks a criterion, changes status, and reopens the saved Markdown task from the list.

### PT-01-C — Verify offline persistence and project isolation

- Depends on PT-01-B.
- Work Integration reviewer. Exercise two offline projects through creation, editing, fast switching, app restart, and missing attachment recovery. Verify against real vault files and the desktop UI.
- Done when the focused Rust suite and `pnpm --filter @next-agent/desktop test:ui` pass, the expected distinct Markdown records are verified after reopen, and desktop and narrow-width UI evidence is recorded for owner review.

## PT-02 · Agent run and review links

### PT-02-A — Store run links by planned task ID

- Depends on PT-01.
- Work Rust and data specialist. Add durable associations in `store.rs` between the planned Markdown ID, originating workspace, and existing execution task IDs. Capture the link when a run is created through `tasks.rs`.
- Done when store fixtures reject cross-project associations, record separate run IDs for retries, keep repeated link attempts idempotent, and list the same runs after restart without creating duplicate Markdown tasks.

### PT-02-B — Launch through the existing agent flow

- Depends on PT-02-A.
- Work Desktop specialist. Add an explicit Run with Agent action using the existing `taskStart` and `trackTask` pipeline, with the selected task and checklist as context. Link each run to its timeline and available diff. Keep provider and approval handling in the established flow.
- Done when an offline scripted test launches and retries a task, reaches the identified run and diff, and shows two run links on one planned task. A provider or launch error leaves the task accessible for retry.

### PT-02-C — Keep run success and approval distinct

- Depends on PT-02-B.
- Work Integration specialist. Show run completion, review action, and planned task status separately. Reuse the review outcomes for Merge, Discard, and Keep, and preserve explicit task completion.
- Done when scripted pass, failure, cancellation, and review fixtures show that successful execution alone never sets Done, accepted review or explicit completion can set Done, and switching projects during a review preserves the original association.

## PT-03 · Task links from Notes and Design

### PT-03-A — Define stable source references

- Depends on PT-01, DW-02, CL-03.
- Work Data specialist. Define project-scoped references for a Notes page and block and a Design board artifact and item. Use the IDs provided by those features, with stable task IDs from Markdown. Specify how missing or moved source items appear and how retries avoid stale writes.
- Done when fixtures round-trip both reference forms and reject forged cross-project links. Broken references remain visible and recoverable without silently creating a task.

### PT-03-B — Link a Notes TODO to a planned task

- Depends on PT-03-A.
- Work Notes specialist. Add an explicit link action at a TODO block in the existing editor and show the linked task's status and destination. Bind writes to the initiating note and project.
- Done when a focused UI test links an existing task, follows the link, and sees its latest status. A project switch with an edit queued cannot modify the other graph or task, and a standalone TODO remains usable.

### PT-03-C — Link a Design board item to a planned task

- Depends on PT-03-A.
- Work Design specialist. Add an explicit link action for a selected item in the DW-02 workspace. Use the stable board and item IDs and the existing board revision check when saving metadata.
- Done when a fixture or UI test shows that the link survives save and reopen and exposes the same task status as Tasks. A stale edit, removed item, or project switch produces a recoverable result without updating another board or task.

## PT-04 · Optional board and Markdown exchange

### PT-04-A — Present status columns from the same task list

- Depends on PT-01, PT-02.
- Work Desktop specialist. Add an optional Kanban view over the same Markdown-backed task data and shared status update action. Support keyboard movement between status columns.
- Done when a focused UI test moves a card by keyboard, returns to the list, and sees the same task status after reopening the project. The vault still contains one record per task.

### PT-04-B — Preview and apply a Markdown import

- Depends on PT-01, PT-02.
- Work Rust and data specialist. Define a documented Markdown exchange fixture. Parse and preview adds, existing IDs, and conflicts without writes. Apply only the confirmed preview to its originating project and preserve stable IDs and acceptance checklists.
- Done when tests show preview is read-only, confirming the same import twice creates no duplicates, a stale or cross-project preview is refused, and the existing repository tracker format stays readable.

### PT-04-C — Export and verify the exchange flow

- Depends on PT-04-B.
- Work Desktop and integration specialist. Expose an explicit import preview and export destination. Use the exchange format to export planned tasks with their stable IDs, status, and criteria.
- Done when a focused UI test cancels a preview without writing, confirms a sample, and exports to a user-selected file. A fixture export-to-import round trip preserves task identity and acceptance criteria, and the tracker records the optional scope or approval.
