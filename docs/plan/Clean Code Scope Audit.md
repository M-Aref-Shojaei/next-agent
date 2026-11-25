---
title: Clean Code Scope Audit
tags: [plan, review, refactoring]
---

# Clean Code Scope Audit

The audit covers RF-01-A through RF-01-C against `fe8c7bd`. The [feature consolidation audit](Feature%20Consolidation%20Audit.md) already assigns navigation, project-vault, Design, and provider consolidation to feature work. The [earlier clean-code review](Clean%20Code%20Review%202026-11-15.md) records larger refactors and unresolved behavior decisions.

| Reviewer | Model and effort | Assignment |
|---|---|---|
| Desktop reviewer | GPT-6.2 Sol, high | Trace Svelte and TypeScript callers and compare state transitions. |
| Rust reviewer | GPT-6.2 Astra, high | Compare Tauri and Rust paths across wire, persisted-state, and permission boundaries. |
| Validation reviewer | GPT-6.2 Sol, low | Confirm focused commands, CI expectations, and the first candidate's behavior and rollback. |

## Candidates

| Scope | Paths and callers | Decision and evidence |
|---|---|---|
| Task-stream message ID | In `apps/desktop/src/lib/app.svelte.ts`, the cron listener at line 76 extracts the ID inline. The channel listener at line 245 uses `messageTaskId` from `apps/desktop/src/lib/timeline.ts:107`. Both handle nested event IDs and top-level message IDs. | **Selected for RF-02.** Reuse the existing helper in the cron listener. Preserve per-source membership checks and task selection behavior. The helper fixture in `apps/desktop/tests/channels.test.ts:70` covers both message shapes. |
| Pending approval count | The workspace status bar reads the all-task `pendingCount` in `app.svelte.ts:48`. The active task timeline uses `pendingApprovals` from `timeline.ts:111`. Both select approvals whose choice is undefined. | Defer to a separate small card if needed. Keep global counting and per-task rendering distinct. Retain the undefined-only predicate and verify settled approvals and multi-task counts. |
| Notes error display | `apps/desktop/src/lib/notes/store.svelte.ts:81,126` repeats the conversion in `apps/desktop/src/lib/errors.ts`. | A later Notes-only change can use the helper. Check string and Error inputs and retain the existing Logseq setup message. Overlay error display strips an additional prefix and diagnostics applies redaction. Both have separate contracts. |
| Desktop runtime detection | `apps/desktop/src/lib/api.ts:500`, `overlay-settings.ts:47`, and the Coucou layout repeat the same runtime predicate. `api.ts` also installs a log sink at import. | Defer to a separate card with a pure predicate and browser/native/no-window fixtures. Keep transport behavior and vendor code independent. |
| Router health notification | `apps/desktop/src-tauri/src/service.rs:275` increments the same watch value as the existing `bump_router_generation` helper at line 160. Gateway setting writes already call the helper. The poller subscribes in `lib.rs:243`. | A later one-file Rust change can use the helper at the current post-write location. Freeze one notification for relevant successful writes and zero for unrelated or rejected writes. Keep provider migration with CL-04. |
| Approval choice parsing | `apps/desktop/src-tauri/src/commands.rs:152` and the Coucou caller in `coucou.rs` converge on `tasks.rs:451`. The parser at `tasks.rs:1008` overlaps with `crates/core/src/broker.rs:69` for canonical labels. | Defer for a dedicated permission review. The desktop currently sends `always` and accepts `always_for_workspace`. A future adapter can normalize the desktop alias while preserving the core parser contract, unknown-input errors, and scoped rule decisions. Test both aliases, each canonical choice, and rejected input before any migration. |
| Persisted execution modes | Desktop `apps/desktop/src-tauri/src/service.rs` and `crates/permission-engine/src/mode.rs` parse related mode names in distinct contexts. | Retain both paths. The desktop wire label and the core restore behavior have different contracts for elevated modes. Reusing one parser across both would change restoration behavior. |

## Feature work boundaries

- CL-01 and CL-02 own the rail, sidebar, palette, and contextual destinations. PS-01 and UX-02 own project opening and setup. Preserve the shared project selection logic and its protection against delayed discovery responses.
- CL-03 owns the difference between project-aware Notes and Boards' current global-vault storage. DW-02 and DW-04 own the board-to-mockup references. Preserve legacy files and keep live project preview and board editing available.
- CL-04 owns the legacy router URL and explicit Providers workflow. Overlay provider settings retain the Coucou-specific workflow.
- Rust project creation already delegates to workspace registration after exclusive folder creation. Discovery reopens and verifies the workspace identity before reading connections. Vault callers also differ today. Board commands require a configured path, Notes opens an explicit graph, and task documentation skips when no vault is available. Preserve those contracts until the project-vault work handles them.
- Task completion assembles the worktree review artifact and can clean up an empty worktree. Explicit review applies Merge, Discard, or Keep. Design already shares the review paths, with different call timing.
- Overlay configuration review presents a hook's line diff. The task DiffViewer presents structured worktree changes with review actions. Keep their action and approval contracts separate.

## RF-02 handoff

GPT-6.2 Sol at low effort is the suggested implementation model for this one-expression change. Keep a separate reviewer for the evidence and diff. Reserve Astra for candidates that affect permission or persisted-state contracts.

The first change has one production file, `apps/desktop/src/lib/app.svelte.ts`. The helper import already exists. Coordinate ownership of that file with any project or task work and branch from current `main` when implementing.

1. **RF-02-A** Record that both the nested event and top-level notice resolve to their own task ID. Recheck that cron messages reach only the adopted cron tasks and that channel messages retain their separate set, origin, and selection rules. The existing unit tests verify helper output and timeline behavior. The source review covers handler membership and selection. Use the focused test command below again on the implementation base.
2. **RF-02-B** Replace the inline cron expression with `messageTaskId(message)`. Keep the surrounding stream callbacks, startup error handling, task membership sets, and direct Chat/Design streams as they are.
3. **RF-02-C** Verify that the duplicate inline expression is gone and the existing channel code is intact. The caller swap removes the duplicate in the same change. Reverting that commit restores the original expression, with no persisted data or API changes.

## Baseline evidence

Independent readback verified the expression match, the existing helper import, and the one-file change boundary. The listeners subscribe to the same broadcast and use separate membership guards. The baseline covers ID extraction and timeline behavior. AppStore listener dispatch was reviewed in source and has no focused automated fixture in this audit.

- `node --test apps/desktop/tests/channels.test.ts apps/desktop/tests/timeline.test.ts` passed 18 tests.
- `pnpm verify` passed after installing from the frozen lockfile. The Node suite passed 514 tests and skipped 7. Workspace, Rust structure, policy parity, boundary, and capability checks passed.
- The refactor itself remains queued. For the implementation, repeat the focused checks and `pnpm verify`, then run the desktop type check and required PR CI. Add targeted regression coverage if the work moves beyond the one-expression substitution.
