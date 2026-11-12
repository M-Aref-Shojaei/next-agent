---
title: Event Contracts
tags: [architecture, contracts]
status: draft
---

# Event Contracts

Rust types in `crates/core` are the source of truth; TypeScript types and JSON Schemas are generated into `packages/contracts` (candidate tools: `specta` + `tauri-specta`, or `ts-rs` + `schemars`; decide in M0).

## Envelope
```json
{ "v": 1, "id": "evt_…", "task_id": "tsk_…", "seq": 42, "ts": "2026-11-10T10:00:00Z", "kind": "tool_requested", "data": { } }
```
- `seq` monotonically increasing per task; UI reorders/dedupes by `(task_id, seq)`.
- `v` bumps on breaking change; sidecars declare supported versions at handshake.

## Kinds (M1)
| kind | data |
|---|---|
| `task_started` | mode, executor, model, workspace, worktree? |
| `message_delta` / `message` | role, text |
| `plan_proposed` | markdown plan (plan mode) |
| `tool_requested` | tool, args, call_id |
| `permission_decided` | call_id, decision, reason, rule_id? |
| `approval_requested` | approval_id, call_id, summary, options |
| `approval_resolved` | approval_id, choice, by |
| `tool_output` | call_id, stream (stdout/stderr), chunk |
| `tool_finished` | call_id, ok, exit_code?, summary |
| `worktree_created` | path, branch, base |
| `diff_ready` | files changed, stats |
| `mode_changed` | from, to, by |
| `task_completed` / `task_failed` / `task_cancelled` | summary / error |
| `vault_record_written` | path |

## Transport
- UI: Tauri `Channel<AgentEvent>` per subscribed task; commands via `invoke`.
- Sidecars: JSON-RPC 2.0 over stdio (newline-delimited), same envelope.
