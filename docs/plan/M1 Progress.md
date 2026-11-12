---
title: M1 Progress
tags: [plan, progress]
status: live
---

# M1 Progress

Task ledger for [[M1 Vertical Slice]]. Update this note (and the step boxes there) when a task is done, with its tests and any deviation.

## Status

| Task | Status | Tests | Notes |
|---|---|---|---|
| 0. Scaffold | ✅ done | 13 TS tests + workspace check | Rust not installed; see [[Development Setup]] |
| 1. Workspace setup & SQLite | ✅ done (code) | 5 Rust unit + 5 integration tests | Rust not compiled here — pending toolchain |
| 2. Agent execution abstraction | ⏳ next | — | |
| 3. Permission engine | ⏸ | — | |
| 4. Git worktree isolation | ⏸ | — | |
| 5. Vault task documentation | ⏸ | — | |
| 6. 9Router endpoint & health | ⏸ | — | |
| 7. Workstation UI | ⏸ | — | |
| 10. CI matrix | ⏸ deferred | — | Needs GitHub remote + network |

## Log

### 2026-11-10 — Task 0: Scaffold
- Created the Cargo workspace (`core`, `permission-engine`, `process-manager`, `apps/desktop/src-tauri`) and the pnpm workspace layout (`packages`, `services` stubs).
- Added `packages/contracts` (event envelope + validator) and `packages/vault` (frontmatter, wikilinks, slugs, record paths) with node:test suites.
- Added `apps/desktop/src/lib/modes.ts` (Shift+Tab cycling, labels, tones) with tests.
- **Deviation:** `rustup` and `pnpm` could not be installed — the machine had no outbound DNS. Tests therefore run on Node 24's built-in runner and native TypeScript support (`node scripts/test.mjs`), with zero dependencies. `cargo test` is wired in `Makefile` and `package.json` but has not run.
- **Deviation:** the Tauri app is a Rust library with constants only; the real `tauri` dependency and window come with the UI task once crates can be fetched.

### 2026-11-10 — Task 1: Workspace setup & SQLite metadata
- `crates/core/src/workspace.rs`: `Workspace::open` (canonical root, deterministic `ws_` id), lenient canonicalization for not-yet-existing files, lexical normalization, `contains` with symlink-escape rejection.
- `crates/core/src/store.rs`: `Store` over `rusqlite` (bundled) with the M1 schema — workspaces, vaults, workspace_vaults, sessions, tasks (mode/status CHECK constraints), events, settings. WAL + foreign keys via `pragma_update`.
- API: `open`, `open_in_memory`, `schema_version`, `upsert_workspace`, `list_workspaces`, `get_setting`, `set_setting`.
- Tests: migrations idempotent, reopen-a-file keeps data, upsert idempotent, settings overwrite, invalid mode rejected, FK cascade deletes tasks.
- **Deviation:** `rustc`/`cargo` are absent, so these tests have not been executed yet. Reviewed by hand against the `rusqlite` 0.32 API (`pragma_update`, `params!`, `query_map`) to keep compile risk low. Run `cargo test --workspace` once the toolchain is installed.
