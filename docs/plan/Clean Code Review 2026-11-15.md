# Clean Code Review 2026-11-15

Seven read-only reviewers, one per area: router, learning and store, core engine, channels and design, the graph/panel/process crates, the desktop backend, and the web frontend. They listed 95 small tasks. 69 of those are safe for a cheap model and are being fixed one commit per task on separate branches. The web-frontend tasks are folded into the per-page design-system restyle.

This note lists what is **not** a cheap task: bugs that need judgement, and decisions for the owner.

## Bugs found (need a careful fix, not a cheap model)

| Where | What happens | Proposed fix |
|---|---|---|
| `crates/core/src/router/gateway/mod.rs:~1421` | A local keychain read error counts as a provider failure, so it can open that provider's circuit breaker. | Map it to a config error that doesn't touch the breaker; add a test. |
| `crates/channels/src/hub.rs:~460` (`request_pairing`) | A storage read error is treated as "no pending codes", so the new code overwrites every other pending one. | On a read error, log it and refuse (`Full`) without writing. |
| `crates/core/src/broker.rs:~254` | A failed audit write is silently dropped, so a denial can leave no audit record. | Decide between fail-closed and logging (see the decisions below). |
| `crates/core/src/broker.rs:~389` | A call cancelled before it started is recorded as `allow`, although it never ran. | Record it as `cancelled`/`deny`. |
| `apps/desktop/src-tauri/src/tasks.rs:~439` | The vault write error after a merge is dropped (`let _`). | Show a notice. |
| `apps/desktop/src-tauri/src/tasks.rs:~308` | Per-step undo is silently off when snapshots fail to open (`.ok()`). | Show a notice that undo is off for this task. |
| `apps/desktop/src-tauri/src/learning.rs:~447` | A failed transcript recorder is dropped silently. | Log it. |
| `crates/core/src/learning/cron.rs:~771` | A DB error while claiming a cron slot is read as "already claimed", and run results are discarded on error. | Log errors (minimum). |
| `crates/macos-panel/src/sys.rs` (`post_paste`) | If key-up fails after key-down, Command can stay held. | Always post key-up; needs an input-safety review. |
| `crates/core/src/router/gateway/mod.rs:1056` vs `:1367` | The cost estimate uses a 16,384-token ceiling for Anthropic while requests use 8,192. | Use one value. |

## Decisions for the owner

1. **Audit-write failure:** should a task stop when its audit log can't be written (fail closed), or carry on and log it?
2. **Channels on storage errors:** outbound chunk dedupe and inbound dedupe currently fail **open** (they may resend or reprocess). Keep that, or fail closed?

## Bigger refactors (later; each needs its own reviewed task)

- **Security-reviewed splits:** `SqliteBroker::handle_inner`, the permission engine's long parsers, the design preview proxy `handle()`, and the channel hub `handle()`/`supervise()`.
- `store.rs` (3.7k lines): move each `// --- block ---` into its own module. No schema change.
- Learning: split `skills::run_op`, `sessions::discover` and `memory::try_apply` per operation.
- Desktop: a shared `map_err` extension trait (about 90 sites); an args struct for `design_generate`.
- Frontend: split `mock-api.ts` by domain; extract components from the Design (729 lines) and Boards (472 lines) pages.
- One shared HTTP client for `fetch.rs` and `catalog.rs`.
