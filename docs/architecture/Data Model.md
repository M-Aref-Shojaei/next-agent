---
title: Data Model
tags: [architecture, storage]
status: draft
---

# Data Model

SQLite (WAL) at `<app_data>/next-agent.db`. See [[ADR-0003 SQLite Metadata, Markdown Source of Truth]]. Schema version 1 is implemented in `crates/core/src/store.rs`; the tables below marked *(M1 step 3)* are not created yet.

```sql
workspaces(id TEXT PK, path TEXT UNIQUE, name, created_at, last_opened_at, default_mode)
vaults(id TEXT PK, path TEXT UNIQUE, name, created_at)
workspace_vaults(workspace_id, vault_id, subfolder)
sessions(id PK, workspace_id, mode, created_at)
tasks(id PK, session_id, workspace_id, title, prompt, mode, executor, model,
      status, base_commit, worktree_path, branch, created_at, finished_at, vault_record_path)
events(task_id, seq, id, kind, data JSON, ts, PRIMARY KEY(task_id, seq))
-- (M1 step 3)
permission_rules(id PK, scope TEXT CHECK(scope IN ('global','workspace')), workspace_id,
      effect TEXT CHECK(effect IN ('allow','deny')), kind, pattern JSON, created_at, created_by)
-- (M1 step 3)
audit_log(seq INTEGER PK AUTOINCREMENT, ts, task_id, mode, action, target, decision,
      reason, actor, prev_hash BLOB, hash BLOB)
settings(key PK, value JSON)
```

## Audit log integrity
- Append-only: triggers `BEFORE UPDATE/DELETE ON audit_log → RAISE(ABORT)`.
- Hash chain: `hash = SHA-256(prev_hash || canonical_json(row))`; verifier command in Settings and CLI (`next-agent audit verify`).
- Honest limitation: a local admin can rewrite the file; optional periodic export/anchor (M2+).

## Retention
`events` prunable per task (user setting); `audit_log` never pruned automatically, exportable as JSONL.
