---
title: M1 Vertical Slice
tags: [plan, milestone]
status: draft
---

# M1 — Vertical Slice

Steps are ordered by dependency. Each step lists **deliverables**, **acceptance criteria**, and **tests**. Check items off as they land.

## Step 0 — Scaffold (M0 carry-over)
- [x] `git init`, `.gitignore`, `.editorconfig`, licence choice ([[Risks and Open Questions]] Q1).
- [x] Cargo workspace: `crates/core`, `crates/permission-engine`, `crates/process-manager`, `apps/desktop/src-tauri`.
- [x] pnpm workspace: `apps/desktop` (SvelteKit + Vite, `adapter-static`), `packages/contracts`, `packages/ui`, `packages/vault`, `services/*` stubs with READMEs.
- [ ] CI matrix (macos-latest, windows-latest, ubuntu-22.04): fmt, clippy `-D warnings`, test, `pnpm check`, `tauri build --debug`.
- **Accept (revised):** `node scripts/test.mjs` and `node scripts/check-workspace.mjs` pass; `cargo test` pending toolchain. See deviations in [[M1 Progress]].

## Step 1 — Workspace setup & SQLite metadata
- [x] `crates/core`: domain type `Workspace` (`Task`/`Session` rows modelled in SQL; typed structs follow with the orchestrator).
- [x] SQLite via `rusqlite` (bundled), embedded idempotent SQL migrations, WAL mode, FK enforcement.
- [x] Schema per [[Data Model]]: `workspaces`, `vaults`, `workspace_vaults`, `sessions`, `tasks`, `events`, `settings` (rules + audit tables land with the permission engine).
- [x] Tauri commands: `workspace_open(path)`, `workspace_list`, `vault_set(path)`.
- [x] Canonicalize workspace root on open (resolve symlinks); reject non-directories.
- **Accept:** opening a folder persists it; restart shows it in recents.
- **Tests:** migration from empty, file reopen, upsert idempotency, settings round-trip, CHECK constraints, FK cascade. Tauri command wrappers still TODO.

## Step 2 — Agent execution abstraction
- [x] `AgentExecutor` trait in `crates/core` ([[Agent Execution]]): `start(task, ctx) -> stream<AgentEvent>`, `cancel(task_id)`, `respond(approval_id, decision)`.
- [x] **Tool broker** in Rust: agents emit `ToolRequest`; broker asks the permission engine, executes, returns `ToolResult`. Tools for M1: `read_file`, `list_dir`, `search` (ripgrep crate `grep`/`ignore`), `write_file`, `apply_patch`, `run_command`, `http_fetch`.
- [x] `BuiltinExecutor`: minimal tool-calling loop against the OpenAI-compatible router ([[ADR-0006 Built-in Tool Loop First, Upstream Engines Behind AgentExecutor]]).
- [x] `MockExecutor` with scripted tool calls for tests and UI development without an LLM.
- [x] Events persisted to `events` and forwarded to the UI via Tauri `Channel`.
- **Accept:** MockExecutor script runs through broker and appears in the timeline in order.
- **Tests:** event ordering, cancel mid-tool, persistence of every event.

## Step 3 — Permission engine
- [x] `crates/permission-engine` — pure, I/O-free `evaluate(request, mode, rules, ctx) -> Decision` ([[Permission Engine]]).
- [x] Path boundary checker: normalize, canonicalize nearest existing ancestor, reject `..`/symlink/junction escapes, case-insensitive on Windows/macOS, sensitive-path list (`~/.ssh`, `~/.aws`, `~/.config`, `/etc`, keychains…).
- [x] Command classifier: parse with shell-aware tokenizer (split on `|`, `&&`, `;`, `$( )`); classes `safe_dev`, `unknown`, `sensitive`, `network_remote`, `deploy`, `catastrophic`.
- [x] Rule store: `Deny` > catastrophic > mode logic > `Allow` rules; "Always for this workspace" writes a scoped allow rule.
- [x] Mode state machine; `Shift+Tab` cycles `plan → manual → auto`; `bypass` only via confirm dialog or `--dangerously-skip-permissions`.
- [x] Approval round-trip: `Decision::Ask` pauses the tool, emits `ApprovalRequested`, resumes on `Allow`/`Deny`/`AlwaysForWorkspace`.
- [x] Audit: every decision logged; bypass/out-of-sandbox actions in hash-chained `audit_log`.
- **Accept:** decision table in [[Permission Engine]] fully covered by table-driven tests.
- **Tests:** path-escape corpus (≥50 cases incl. symlinks, `\\?\` paths, UNC, `~`), classifier corpus, property tests (`proptest`) that `Deny` always wins, audit chain verification.

## Step 4 — Git worktree isolation
- [ ] On first mutating tool in `auto` mode: `git worktree add .next-agent/worktrees/<id> -b next-agent/auto-<short-id>` from current HEAD ([[Worktree Isolation]]).
- [ ] Sandbox root for the task becomes the worktree path.
- [x] Diff API: `git diff main...next-agent/auto-*` → structured hunks for the UI.
- [x] Merge actions: merge (no-ff or squash, configurable), discard (remove worktree + branch), keep for later.
- [x] Refuse auto mode on non-git workspaces (fall back to `manual` with explanation) — see Q4 in [[Risks and Open Questions]].
- **Accept:** auto task never touches the main working tree until the user merges.
- **Tests:** temp-repo integration tests; conflict on merge surfaces cleanly; dirty main tree handling.

## Step 5 — Vault task documentation
- [x] `packages/vault` (TS) for parsing/indexing + Rust writer in core for atomic writes (temp file + rename).
- [ ] On task completion write `Tasks/YYYY/MM/YYYY-MM-DD-<slug>.md` using [[Task Record Template]]; append link to daily journal `Journal/YYYY-MM-DD.md`.
- [ ] Summary generated by LLM from events; fall back to a deterministic summary if router unavailable.
- [x] Vault writes are internal (not agent tool calls) but still audited.
- **Accept:** each finished task produces one record with frontmatter, files changed, commands run, decisions, outcome.
- **Tests:** golden-file tests for rendering; idempotent re-run doesn't duplicate.

## Step 6 — 9Router endpoint & health
- [x] Settings: base URL (default `http://127.0.0.1:20128/v1`), API key (OS keychain via `keyring` crate), default model.
- [x] Health check: `GET /v1/models` with timeout; status `ok | degraded | down` polled by the process manager ([[Router Integration]]).
- [x] Streaming chat completions (SSE) client in Rust (`reqwest` + `eventsource-stream`).
- [x] Router calls are **not** subject to network permission prompts (they're the app's own LLM channel) but are logged.
- **Accept:** status dot in the UI reflects router up/down within 5 s.
- **Tests:** mock server (`wiremock`) for ok/timeout/401/streaming.

## Step 7 — Workstation UI
- [x] Layout ([[Desktop UI]]): left sidebar (workspaces, tasks), centre chat timeline, right panel (diff / files / vault), bottom status bar.
- [ ] Status bar: mode badge (`Plan`/`Manual`/`Auto`/`Bypass` with distinct colours; Bypass red), router health, branch/worktree.
- [x] `Shift+Tab` mode cycling (only when focus isn't in a text field that needs it — decision in Q6).
- [x] Timeline: messages, tool calls with collapsible output, inline approval cards (Allow / Deny / Always for this workspace).
- [ ] Side-by-side diff reviewer (CodeMirror 6 merge view) with Merge / Discard / Keep.
- [x] Settings: router, vault path, permission rules editor, audit log viewer (read-only).
- **Accept:** full exit scenario from [[Roadmap]] runs end-to-end via UI.
- **Tests:** Vitest component tests; Playwright/WebDriver smoke on Linux CI.

## Definition of done (M1)
- All steps accepted on macOS, Windows, Linux.
- Security review of permission engine & path checker done.
- Docs in this vault updated; ADRs accepted.
