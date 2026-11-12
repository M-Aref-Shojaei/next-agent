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
| 0. Scaffold | ✅ done | node + Rust suites run locally | Rust 1.100 and pnpm 10 installed 2026-11-12 |
| 1. Workspace setup & SQLite | ✅ done | store + desktop command tests | Tauri commands `workspace_open`/`workspace_list`/`vault_set` landed with the UI |
| 2. Agent execution abstraction | ✅ done (module) | execution + broker tests | Desktop still drives the legacy `run_task` path, not `spawn_task` |
| 3. Permission engine | ✅ done | 53 engine tests + shared path/command/policy corpora | Security review pending (M1 DoD) |
| 4. Git worktree isolation | 🟡 API done | 19 temp-repo tests | Not yet triggered by the broker on the first mutating auto tool |
| 5. Vault task documentation | 🟡 writer done | Rust + TS golden tests | Not yet called on task completion; LLM summarizer not plugged in |
| 6. 9Router endpoint & health | ✅ done (module) | 17 wiremock router tests | UI status dot still stubbed (HealthMonitor not wired) |
| 7. Workstation UI | 🟡 shell done | desktop Rust + node tests, svelte-check clean | Diff reviewer stubbed; GUI not launched yet |
| 10. CI matrix | 🟡 written | YAML parses | Never run: no GitHub remote |

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

### 2026-11-12 — Parallel build-out of Steps 0, 2–7 and integration
- Installed Rust stable (rustup) and pnpm (corepack); first real `cargo test` run. Baseline fixes: slice iterator in rules, `upsert_workspace` insert/update detection, policy-parity test on paths with spaces.
- Six work sessions, one branch each, merged into `main`: permission engine (corpora, rule precedence, `always_rule`, `explain_with`), agent execution + router (`spawn_task`, `LocalTools`, `BuiltinExecutor`, `RouterClient`, `HealthMonitor`), worktree isolation (`crates/core/src/worktree.rs`), vault writer (`crates/core/src/vault.rs`), Tauri 2 desktop shell, CI workflow.
- Integration: vault and desktop updated for the merged event/tool contracts; desktop envelopes now come from `EventKind::data()` with the sequenced timestamp; broker records approval answers in the event timeline when the approver doesn't.
- **Security fix:** "Always for this workspace" used to persist a tool-wide allow (one approval let every later command run). It now stores the exact argv or canonical path via `always_rule`; compound commands and network requests are one-time. Policy runs with real filesystem facts (`RealFs`) and file tools act on the resolved path. Regression tests added.
- State: 234 Rust tests, 207 node tests, clippy `-D warnings`, fmt, `make check`, svelte-check and the frontend build all pass.
- **Next:** wire the exit scenario end-to-end in the desktop app — `spawn_task` + approvals, worktree on first mutating auto tool, diff reviewer, vault record on completion, live router health — then launch the GUI and push CI to a remote.
