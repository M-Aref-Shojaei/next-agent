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
| 2. Agent execution abstraction | ✅ done | execution + broker tests | Desktop runs tasks through `spawn_task` |
| 3. Permission engine | ✅ done | 53 engine tests + shared path/command/policy corpora | Security review pending (M1 DoD) |
| 4. Git worktree isolation | ✅ done | 19 temp-repo tests + desktop end-to-end | Worktree is created at task start, not on the first mutating tool (see log) |
| 5. Vault task documentation | ✅ done | Rust + TS golden tests + desktop end-to-end | LLM summarizer not plugged in; deterministic summary used |
| 6. 9Router endpoint & health | ✅ done | 17 wiremock router tests | Status bar shows live health |
| 7. Workstation UI | 🟡 wired | desktop Rust + node tests, svelte-check clean | Exit scenario wired; diff view is a plain side-by-side table, not CodeMirror; no Playwright smoke yet |
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

### 2026-11-13 — Exit scenario wired in the desktop app
- Desktop tasks now run through `spawn_task` with `LocalTools`: approvals come from `approval_requested` events and are answered per task (`approval_respond(task_id, approval_id, choice)`); cancel uses `TaskControl`.
- Auto mode creates `next-agent/auto-*` worktrees via `GitCli` and makes the worktree the sandbox root. Non-git folders, or repos with no commits, fall back to Manual with a notice in the timeline (Q4).
- **Deviation:** the worktree is created when the task starts, not on its first mutating tool. A task that changes nothing has its worktree removed at the end.
- When a task finishes, the app checkpoints its worktree, sends the diff to the reviewer and writes the vault record. The reviewer offers Merge (squash by default, Q5), Discard and Keep. Merging rewrites the record with `merged: true`.
- Agent: `BuiltinExecutor` when the router is healthy and a default model is set; otherwise the scripted demo agent, with a notice saying which one ran.
- Router health streams to the status bar through a `router-health` event. The poller rebuilds its client when the router settings change. The UI setting keys are mapped to the core's `router.base_url` and `router.default_model`.
- Tests: 236 Rust, 211 node; clippy, `make check` and svelte-check are clean. Desktop end-to-end tests cover auto → worktree → review → vault record → merge, discard, and the non-git fallback to Manual with a denied approval. `tauri dev` launches. The window was not screenshotted (no screen-recording permission); the frontend was smoke-tested in a browser against the mock API.
- **Left for M1 DoD:** CodeMirror merge view, an LLM summarizer for task records, a Playwright/WebDriver smoke test, a real-router run, a security review, and CI on a remote across all three OSes.

### 2026-11-14 — Wave 1 of the built-in upstreams merged ([[Built-in Upstreams Plan]])
- **LLM gateway** (from [[9Router]], [[OmniRoute]], [[OpenRouter]] docs):
  - Providers: OpenAI-compatible, Anthropic, Gemini; keys in the keychain; translation between their formats.
  - Combos: fallback, round-robin, smart, weighted, cost-optimized, last-good, context-optimized.
  - Token savers: Caveman, Ponytail, RTK, Anthropic cache_control.
  - Usage and cost tracking, cache-aware.
  - Model roles; budget caps; exact-match response cache; per-provider circuit breaker; prompt-cache affinity.
  - Model catalog import (manual); one reasoning setting across providers; context-overflow model switch.
  - Real chat no longer needs an external router.
- **Agent engine** (from [[opencode]], [[Hermes Agent]]):
  - Context compaction; tiered edit matching; shadow-git per-step undo.
  - Markdown agent profiles; one subagent tool; prompt-cache invariants.
- **Notes engine** (from [[Logseq]]):
  - `crates/graph`: Logseq Markdown parser and writer with byte-for-byte round-trip, and a SQLite index (FTS5, refs, backlinks) with a file watcher.
  - Task records and journals now use the Logseq layout.
  - `graph_*` commands are ready for the Wave 2 UI.
- **Overlay** (from [[Coucou]], code ideas only, no assets):
  - Always-on-top island with click-through and an approval queue.
  - Notch placement on macOS; `crates/macos-panel` is the only crate allowed `unsafe`.
- **Design board** (Excalidraw):
  - Boards in `vault/boards/*.excalidraw`, with a wireframe library and templates.
  - Mermaid import; PNG and SVG export; a sketch hand-off API for the future Design Studio.
- **Integration** (coordinator):
  - Schema v3.
  - Gateway token counts feed compaction.
  - Desktop wiring: undo button, agent profile picker, summary role for compaction, context window from the catalog.
  - The topology generator ignores empty untracked folders.
- **Reviews:**
  - Gateway security review found 2 high, 3 medium and 6 low issues, all fixed. The cache no longer stores truncated answers and keys entries per connection. Budgets fail closed and reserve spend before a request.
  - Board code review found 1 high (autosave could wipe a board), 4 medium and several low issues, all fixed. Also fixed: Windows reserved names, symlink escapes, no-clobber writes, licence rows.
- **Tests:** 537 Rust and 243 node pass; clippy, fmt, `make check`, svelte-check and the build are clean.
- **Not verified:** a real-provider chat with a live API key. Overlay behaviour on real monitors. Excalidraw's `blob:` workers inside the Tauri webview (checked only in a browser).
- **Deferred** (from the agent-engine handover):
  - Put the profile floor on `AgentStep::Call`.
  - An `LlmError::ContextOverflow` type.
  - One tool table.
  - Snapshots inside `TaskLaunch`.
  - Undo skips untracked files over 2 MiB.
  - Warn when two tasks share one work tree.

