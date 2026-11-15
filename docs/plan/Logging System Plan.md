---
title: Logging System Plan
tags: [plan, logging, privacy]
status: proposed
---

# Logging System Plan

**Goal:** one local diagnostics log for the whole app. It records **how the app behaves**: what started, what failed, how long things took, and which component and setting were involved. It never records **what people work on**: no prompts, AI answers, note or file contents, chat messages, task titles, keys or tokens. The decision and its reasons are in [[ADR-0013 Local Diagnostics Log Without User Content]].

> Owner's words (2026-11-15): "any thing would happened in the app should be logged but not from the context of work people on it, about the functionality of app".

## Today (surveyed 2026-11-15)
- **Rust crates:** 37 `tracing` calls across `crates/core` (router, broker, events, learning) and `crates/channels`. **No subscriber is installed anywhere, so all of them are thrown away.** Some pass free-text errors (`%error`) that may echo provider response bodies.
- **Desktop backend:** 9 `eprintln!` calls in `upstreams.rs`, `gateway.rs` and `coucou.rs`. They are visible only in a terminal.
- **Coucou overlay:** 156 `log::line(...)` calls write their own free-text file, `overlay/next-agent.log` (1 MB cap, reset when full). Not every line has been checked for user content.
- **Frontend:** 6 `console.*` calls. Window errors and unhandled promise rejections are lost.
- **Panics:** no hook, so a panic leaves no record.

## Design

### Where and how
- **Files:** `<app data>/logs/next-agent-YYYY-MM-DD.jsonl`, for example `~/Library/Application Support/dev.nextagent.desktop/logs/`.
  - One JSON object per line.
  - Rolled daily, and also when a file passes 10 MB.
  - Kept for **14 days**, and at most **100 MB** in total; the oldest file is deleted first.
  - File mode 0600, folder 0700.
- **Nothing leaves the machine.** No network sink, ever. "Export" writes a file the owner can attach to a bug report themselves.
- **Library:** `tracing` and `tracing-subscriber`, with a file writer (`tracing-appender` or a small own roller), plus an in-memory ring buffer of the last 2,000 events for the Diagnostics page.
- **Levels:** `error`, `warn`, `info` (default) and `debug`. The level can be changed at runtime from Settings.
  - `debug` is off by default. When it is on, the Diagnostics page shows a banner saying so.
- **Development builds** also print to stderr.

### Event shape
```json
{"ts":"2026-11-15T16:34:01.123Z","level":"warn","target":"router","event":"provider_call_failed",
 "fields":{"provider":"anthropic","model":"claude-sonnet-4-5","status":529,"duration_ms":812,"attempt":2},
 "span":{"task_id":"t_01J…"}}
```
- **`event`:** a stable snake_case name, also used for filtering. Free-text `message` is optional and is redacted.
- **`target`:** the area. One of `app`, `router`, `agent`, `broker`, `tasks`, `store`, `worktree`, `vault`, `notes`, `boards`, `design`, `channels`, `learning`, `cron`, `overlay`, `upstreams`, `updates` or `ui`.
- **Ids:** task, approval, channel and cron ids are random identifiers, not content, so they may appear. That lets one task be followed across events.

### Privacy rules (the core of this plan)
1. **Allowlisted fields only.** A subscriber layer writes only field names on an allowlist kept in one file (`crates/diagnostics/src/fields.rs`). Any other field is dropped, and the event gets `dropped_fields: N`. That way a mistake loses detail instead of leaking content.
   - **Starting allowlist:** `error`, `kind`, `code`, `status`, `duration_ms`, `attempt`, `count`, `bytes`, `tokens_in`, `tokens_out`, `tokens_cached`, `cost_micros`, `provider`, `model`, `combo`, `role`, `mode`, `tool`, `decision`, `rule_kind`, `command`, `ok`, `task_id`, `approval_id`, `channel_id`, `channel_kind`, `cron_id`, `session_id`, `window`, `page`, `setting` (the key, never its value), `version`, `from_version`, `to_version`, `migration`, `os`, `arch`, `level`, `reason`, `retries`, `queue`, `files` (a count), `exit_code`, `signal`, `port`.
2. **Forbidden names.** These names can never be added to the allowlist, and a build check fails if code uses them in a log call:
   - `prompt`, `content`, `text`, `body`, `answer`, `response`, `message_text`, `title`, `name` (when it is a user's page, board, skill or file name), `note`, `query`, `args`, `input`, `output`, `diff`, `path` (raw);
   - `token`, `key`, `secret`, `password`, `cookie`, `authorization`, `sender`, `username`, `email`, `phone`.
3. **Redaction of every string value**, including `error` and `message`:
   - **Secret shapes become `[secret]`:** `sk-…`, `sk-ant-…`, `AIza…`, `ghp_…` and `github_pat_…`, `xox…`, Telegram bot tokens (`\d+:[A-Za-z0-9_-]{35}`), `Bearer …`, JWTs, and long hex or base64 runs.
   - **Home folder becomes `~`.** A full path under a workspace or vault is shortened to its file extension and depth, for example `<workspace>/…/*.rs (depth 3)`.
   - **Emails and phone numbers** become `[redacted]`.
   - **Length:** strings are cut to 300 characters.
4. **Provider and HTTP error bodies are never logged.** Only the status, the provider's error *type* or code, and the byte count are kept, because a body can echo the prompt.
5. **Tool calls are logged by tool name and decision only** (`tool=run_command decision=ask`). Their arguments (commands, paths, URLs) are never logged.
6. **Settings are logged by key only:** `setting="gateway.default_model" changed`, never the value.
7. **Panics:** a panic hook logs the crate, file, line and redacted message, and flushes the file before the process ends.
8. **Canary tests prove it.** A test drives sentinel strings (`CANARY-PROMPT-7f3a`, a fake `sk-ant-` key, a fake bot token, a note text) through the gateway, a task, a channel message and a notes save, with logging at `debug`. It asserts that none of them appear in the output.

### What gets logged (the event catalogue)
| Area | Events (at least) |
|---|---|
| app | `app_started` (version, os, arch), `app_stopping`, `panic`, `migration_applied`, `setting_changed` (key), `window_opened`/`closed` |
| ui | `ui_error` (window error or unhandled rejection: kind, redacted message, page), `command_failed` (command, duration_ms, redacted error), `slow_command` (over 2 s) |
| router | `provider_call` (provider, model, status, duration_ms, tokens, cached), `provider_call_failed`, `breaker_opened`/`closed`, `budget_blocked`/`warned`, `cache_hit`, `combo_switch` (from, to, reason), `catalog_refreshed` (count) |
| tasks/agent | `task_started` (task_id, mode, profile), `task_finished` (status, duration_ms, steps, tool calls by tool), `compaction` (tokens before and after), `subagent_started`/`finished`, `undo_step` |
| broker | `tool_decision` (tool, decision, rule_kind), `approval_requested`/`answered` (by: desktop, overlay, timeout), `rule_added`/`removed` (kind only) |
| worktree/vault | `worktree_created`/`merged`/`discarded`, `checkpoint`, `vault_record_written`/`failed` |
| store | `db_opened` (schema_version), `db_error` (kind) |
| notes/boards/design | `graph_indexed` (files, duration_ms), `watcher_error`, `board_saved`/`conflict`, `design_generated` (count, duration_ms), `preview_started`/`stopped` (port), `preview_blocked` (reason) |
| channels | `channel_started`/`stopped`/`health` (status), `pairing_requested`/`resolved` (no codes or sender ids), `message_in`/`out` (counts and bytes only), `api_error` (status) |
| learning/cron | `memory_saved` (target, bytes), `skill_changed` (op, no name), `curator_run` (counts), `cron_fired`/`skipped`/`failed` (cron_id, reason) |
| overlay | `relay_connection` (accepted or refused, reason), `hook_event` (event kind), `overlay_window` (label, op), `dictation` (state only), `phone_link` (state) |
| upstreams/updates | `upstream_check` (status, new commits count), `update_check`/`downloaded`/`failed` |

## Interfaces (fixed here so sessions can work in parallel)

**Rust crate `crates/diagnostics`** (new; the only crate that writes log files):
```rust
pub struct Config { pub dir: PathBuf, pub level: Level, pub retention_days: u32, pub max_total_bytes: u64, pub stderr: bool }
pub fn init(config: Config) -> Result<Handle, InitError>;           // installs the global subscriber + panic hook
impl Handle {
    pub fn set_level(&self, level: Level);
    pub fn recent(&self, filter: &RecentFilter) -> Vec<LogEntry>;   // ring buffer, newest first
    pub fn export(&self, to: &Path) -> Result<ExportSummary, String>; // redacted .jsonl + system info header
    pub fn folder(&self) -> &Path;
}
pub fn redact(text: &str) -> String;                                 // shared with the UI path
```
Crates keep using plain `tracing::{info,warn,error,debug}!` with allowlisted fields. They do not depend on `diagnostics`; only the desktop app does.

**Tauri commands** (main window only; generate capabilities):
| Command | Args | Returns |
|---|---|---|
| `diag_recent` | `{ level?: string, target?: string, limit?: number }` | `LogEntry[]` |
| `diag_set_level` | `{ level: "error"\|"warn"\|"info"\|"debug" }` | `void` (also saved as setting `logging.level`) |
| `diag_open_folder` | none | `void` |
| `diag_export` | none | `{ path: string, entries: number }` (written to Downloads) |
| `diag_log` | `{ level, event, fields }` | `void`; for the UI. Rate-limited to 20 per second, fields allowlisted and redacted on the Rust side. |

**Frontend:** `apps/desktop/src/lib/log.ts` exports `log.info|warn|error(event, fields)`. It installs `window.onerror` and `unhandledrejection` handlers once, and wraps `invoke` in `api.ts` to log `command_failed` and `slow_command`. The Coucou windows install the same handlers.

## Work breakdown

### How the work runs
- **One agent per task**, each in its own worktree branch. The coordinating session merges after each wave and runs all checks: `cargo test --workspace`, `make lint`, `make check`, `node scripts/test.mjs`, svelte-check and the build. It then deletes finished agents and worktrees.
- **Every brief names:**
  - the files the agent owns and must not leave;
  - the skills to use: `engineering:code-review` for privacy and security parts, `simplify` at the end, and `anthropic-skills:built-in-browser` for UI checks;
  - the one-design-system rule for UI ([[Design System]], [[ADR-0012 One Design System]]).
- **Model per task:** Opus for the privacy core, Sonnet for judgement-heavy instrumentation and UI, Haiku for mechanical instrumentation against an exact event list.
- **Times** are agent working time.

### Wave 1: foundation (2 agents in parallel)
| # | Task | Model | Owns | Est. | Done when |
|---|---|---|---|---|---|
| 1 | **Diagnostics core.** Write the `crates/diagnostics` crate as specified above:<br>• subscriber: JSON-lines file roller with retention and size cap, ring buffer, runtime level reload, panic hook, stderr in dev;<br>• the field allowlist layer, the forbidden-name list, `redact()`;<br>• the canary unit tests;<br>• `init` called first thing in `apps/desktop/src-tauri/src/lib.rs`, reading `logging.level`;<br>• the five `diag_*` commands and their capabilities (main window only);<br>• `scripts/check-log-fields.mjs` in `make check` (scans Rust `tracing` macros and TS `log.*` calls for forbidden names and non-allowlisted fields), with node tests;<br>• boundary rule: only `crates/diagnostics` writes under `logs/`;<br>• `docs/architecture/Logging.md`: schema, allowlist, how to add an event. | Opus | `crates/diagnostics/**`, `apps/desktop/src-tauri/src/{lib.rs,diagnostics.rs}`, `scripts/check-log-fields.mjs`, `scripts/check-boundaries.mjs`, `Makefile`, `docs/architecture/Logging.md` | 3–4 h | `cargo test -p next-agent-diagnostics` passes, including canary and redaction tests. Starting the app writes `app_started` to `logs/`. The existing 37 `tracing` calls now reach the file; any whose fields aren't allowlisted show `dropped_fields`. `make check` fails on a planted `tracing::info!(prompt = …)`. |
| 2 | **UI side.**<br>• `lib/log.ts` (logger, window error and rejection handlers);<br>• the `invoke` wrapper in `api.ts` (`command_failed`, `slow_command`) and the `mock-api.ts` stand-ins;<br>• **Settings → Diagnostics** page built on the design system: level selector, live recent entries with level and area filters (a `Table`), "Open log folder", "Export for a bug report", and the debug-level banner;<br>• a sidebar or Settings link;<br>• node tests for `log.ts` (rate limiting, field filtering before send). | Sonnet | `apps/desktop/src/lib/log.ts`, `api.ts` (invoke wrapper only), `mock-api.ts`, `routes/(app)/settings/diagnostics/**`, `tests/log.test.ts` | 2 h | The page renders against the mock in light and dark at 375 px. Errors thrown in the UI appear in the list (mock). |

### Wave 2: instrumentation (5 agents in parallel, after Wave 1 is merged)
Each agent adds the events from the catalogue for its area. It uses only allowlisted fields; adding an allowlist entry needs the coordinator's OK and must not be a forbidden name. Each agent also fixes its area's existing calls that drop fields or log free text.

| # | Area | Model | Owns | Est. | Done when |
|---|---|---|---|---|---|
| 3 | **Router.** Provider calls, failures, breaker, budget, cache, combos, catalog. Replace `%error` on provider errors with status plus error type, so bodies are never logged. | Sonnet | `crates/core/src/router/**` | 1.5 h | The canary prompt passed through the mock provider, including an error response that echoes the prompt, never reaches the log. |
| 4 | **Tasks, agent, broker, worktree, vault, store.** Task lifecycle, tool decisions (tool and decision only), approvals, compaction, subagents, undo, worktree ops, vault writes, DB open and migrations. Replace the 9 `eprintln!` calls. | Sonnet | `crates/core/src/{broker,builtin,compaction,control,runner,worktree,vault,store,events}.rs`, `apps/desktop/src-tauri/src/{tasks,commands,gateway,upstreams,review,service}.rs` | 2 h | A mock task logs started, decisions, approvals and finished, with no arguments or titles. |
| 5 | **Channels, learning, cron, notes, boards, design.** The events from the catalogue. Pairing and messages are logged as counts only. | Haiku | `crates/channels/**`, `crates/core/src/learning/**`, `apps/desktop/src-tauri/src/{channels,learning,notes,boards,design}.rs`, `crates/graph/src/**` (watcher and indexer only), `crates/design/src/**` (proxy and preview only) | 1.5 h | Each area emits its catalogue events, and the canary test (a channel message, a notes save) stays clean. |
| 6 | **Coucou overlay.**<br>• Re-implement `log::line` to emit `tracing` events with target `overlay`, keeping its call sites (upstream code).<br>• Audit all 156 messages: list any that carry user content (chat, dictation or notetaker text, transcripts, window titles, URLs) and change those call sites to an event name plus allowlisted fields, marked `// Next Agent:`.<br>• Move the relay and hook messages to `relay_connection` and `hook_event`.<br>• Retire `overlay/next-agent.log`: new lines go to `logs/`, and the old file is deleted on first start.<br>• The separate `next-agent-hook` relay binary logs nothing to disk; check it and keep it that way. | Sonnet | `apps/desktop/coucou/src-tauri/src/**`, `apps/desktop/coucou/hook/src/**` | 2–3 h | No overlay message contains user content (the audit table is in `docs/upstream/Coucou.md`). The relay fix from 2026-11-15 shows up as `relay_connection accepted`. |
| 7 | **Frontend call sites.** Replace the 6 `console.*` calls with `log.*`, log `ui_error` from each page's existing error paths (not the message contents), and install the handlers in the Coucou windows (`routes/coucou/+layout.svelte`). | Haiku | `apps/desktop/src/**` except the files task 2 owns; `apps/desktop/coucou/src/**` (handlers only) | 1 h | No `console.*` is left outside `log.ts`, and svelte-check and the build are clean. |

### Wave 3: proof (1 agent, after Wave 2 is merged)
| # | Task | Model | Est. | Done when |
|---|---|---|---|---|
| 8 | **Privacy audit and end-to-end canary.**<br>• Add an integration test that runs a scripted task, a gateway call (mock provider), a channel message (mock Telegram), a notes save, a memory save and a board save, all with canary strings at `debug` level, then greps every log file for the canaries and secret shapes.<br>• Review every `tracing`/`log.*` call against the rules, using the `engineering:code-review` skill.<br>• Run the real app for a short session and check the log reads well: one line per real happening, nothing noisy at `info`.<br>• Record the result in `docs/architecture/Logging.md`. | Opus | 2 h | The canary test passes in `cargo test --workspace`. The review finds no leaks, or each one is fixed. The log of a 5-minute session is readable at `info`. |

**Total:** 8 agents; about 4 h of wall time with the waves in parallel, about 16 h of agent time.

## Owner decisions (defaults used unless changed)
- **Retention:** 14 days and 100 MB.
- **Default level:** `info`.
- **Location:** the app data folder.
- **Export destination:** Downloads.
- **Crash reports:** stay local; there is no automatic sending, and none is planned.

## Out of scope
- **Analytics and telemetry** to any server. This is a local diagnostics log only.
- **Logging of user actions for their own sake**, such as which page was opened or what was clicked. A page appears only in an error's context.
