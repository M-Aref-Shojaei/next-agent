# Clean Code Review 2026-11-15

Seven read-only reviewers, one per area: router, learning and store, core engine, channels and design, the graph/panel/process crates, the desktop backend, and the web frontend. They listed 95 small tasks. 69 of those are safe for a cheap model and are being fixed one commit per task on separate branches. The web-frontend tasks are folded into the per-page design-system restyle.

This note lists what is **not** a cheap task: bugs that need judgement, and decisions for the owner.

## Bugs found (need a careful fix, not a cheap model)

| Where | What happens | Proposed fix | Status |
|---|---|---|---|
| `crates/core/src/router/gateway/mod.rs:~1421` | A local keychain read error counts as a provider failure, so it can open that provider's circuit breaker. | Map it to a config error that doesn't touch the breaker; add a test. | Fixed in `7ef29cd`: new `KeyUnavailable` outcome releases the breaker probe and moves on (test `keychain_errors_do_not_open_the_circuit_breaker`). |
| `crates/channels/src/hub.rs:~460` (`request_pairing`) | A storage read error is treated as "no pending codes", so the new code overwrites every other pending one. | On a read error, log it and refuse (`Full`) without writing. | Fixed in `4ae4ea9`: new `PairingOutcome::Unavailable` on a read or save error, nothing written (test `pairing_read_errors_refuse_without_overwriting_pending_codes`). |
| `crates/core/src/broker.rs:~254` | A failed audit write is silently dropped, so a denial can leave no audit record. | Decide between fail-closed and logging (see the decisions below). | Open: owner decision 1. |
| `crates/core/src/broker.rs:~389` | A call cancelled before it started is recorded as `allow`, although it never ran. | Record it as `cancelled`/`deny`. | Fixed in `b6fea06`: audited and returned as `deny` with reason "cancelled before the tool started" (test `calls_cancelled_before_they_start_are_not_recorded_as_allowed`). |
| `apps/desktop/src-tauri/src/tasks.rs:~439` | The vault write error after a merge is dropped (`let _`). | Show a notice. | Fixed in `be41ce5` + `93c92df`: `ReviewOutcome.notice` and a warning (test `merge_reports_a_task_record_that_could_not_be_updated`). The web UI does not show `notice` yet. |
| `apps/desktop/src-tauri/src/tasks.rs:~308` | Per-step undo is silently off when snapshots fail to open (`.ok()`). | Show a notice that undo is off for this task. | Fixed in `68c5f94`: timeline notice and a warning (test `a_task_without_step_snapshots_says_undo_is_off`). |
| `apps/desktop/src-tauri/src/learning.rs:~447` | A failed transcript recorder is dropped silently. | Log it. | Fixed in `146b1fc` (`tracing::warn!`). |
| `crates/core/src/learning/cron.rs:~771` | A DB error while claiming a cron slot is read as "already claimed", and run results are discarded on error. | Log errors (minimum). | Fixed in `496572e`: claim and record errors are logged; a failed claim still skips the run (at most once). |
| `crates/macos-panel/src/sys.rs` (`post_paste`) | If key-up fails after key-down, Command can stay held. | Always post key-up; needs an input-safety review. | Fixed in `a9eb165`: both events are built first, then posted together (test `a_key_down_is_never_posted_without_its_key_up`). |
| `crates/core/src/router/gateway/mod.rs:1056` vs `:1367` | The cost estimate uses a 16,384-token ceiling for Anthropic while requests use 8,192. | Use one value. | Fixed in `435ca0a`: one per-provider ceiling feeds both the request and the estimate (test `cost_estimate_uses_each_providers_request_ceiling`). |

Shared helpers from the review follow-up are in `74b6188`.

## Decisions for the owner

1. **Audit-write failure:** should a task stop when its audit log can't be written (fail closed), or carry on and log it?
2. **Channels on storage errors:** outbound chunk dedupe and inbound dedupe currently fail **open** (they may resend or reprocess). Keep that, or fail closed?
3. **Gemini cost estimate:** when the price table has no `max_output` for a Gemini model, the budget estimate now assumes 65,536 output tokens (the ceiling reasoning uses) instead of 16,384. The built-in `gemini-*` row sets 8,192, so this only affects models with an override that leaves `max_output` out. Keep 65,536, or set a lower budget-only default?
4. **Showing the merge notice:** the merge result now carries `notice` when the task record could not be updated. The web UI (`app.svelte.ts` `review()`) needs to show it; that is a frontend change.

## Bigger refactors (later; each needs its own reviewed task)

- **Security-reviewed splits:** `SqliteBroker::handle_inner`, the permission engine's long parsers, the design preview proxy `handle()`, and the channel hub `handle()`/`supervise()`.
- `store.rs` (3.7k lines): move each `// --- block ---` into its own module. No schema change.
- Learning: split `skills::run_op`, `sessions::discover` and `memory::try_apply` per operation.
- Desktop: a shared `map_err` extension trait (about 90 sites); an args struct for `design_generate`.
- Frontend: split `mock-api.ts` by domain; extract components from the Design (729 lines) and Boards (472 lines) pages.
- One shared HTTP client for `fetch.rs` and `catalog.rs`.
