---
title: Logging
tags: [architecture, logging, privacy]
status: implemented (Waves 1–3; audited 2026-11-16)
---

# Logging

One local diagnostics log for the whole app. It records **how the app behaves** (what started, what failed, how long it took, which component and setting were involved) and never **what people work on** (prompts, answers, note or file contents, messages, task titles, keys, tokens). Nothing ever leaves the machine. Decision: [[ADR-0013 Local Diagnostics Log Without User Content]]. Plan: [[Logging System Plan]].

Code: `crates/diagnostics` (the only writer) and `apps/desktop/src-tauri/src/diagnostics.rs` (startup and the `diag_*` commands). Check: `scripts/check-log-fields.mjs` (in `make check`).

## Where the log is

- `<app data>/logs/next-agent-YYYY-MM-DD.jsonl`, for example `~/Library/Application Support/dev.nextagent.desktop/logs/`. One JSON object per line.
- A new file each UTC day, and `next-agent-YYYY-MM-DD.N.jsonl` once a file would pass 10 MB.
- Files older than **14 days** are deleted, then the oldest until the folder is at most **100 MB**. The file being written is never deleted.
- Folder mode 0700, files 0600. Each line is one `write_all` on an unbuffered file, so a crash loses at most the line being written.
- Development builds also print each line to stderr.
- The last **2,000** entries are kept in memory for Settings → Diagnostics.

## Event shape

```json
{"ts":"2026-11-15T16:34:01.123Z","level":"warn","target":"router","event":"provider_call_failed",
 "fields":{"provider":"anthropic","model":"claude-sonnet-4-5","status":529,"duration_ms":812,"attempt":2},
 "span":{"task_id":"t_01J…"}}
```

| Key | Meaning |
|---|---|
| `ts` | UTC, milliseconds |
| `level` | `error`, `warn`, `info` or `debug` |
| `target` | The area: `app`, `router`, `agent`, `broker`, `tasks`, `store`, `worktree`, `vault`, `notes`, `boards`, `design`, `channels`, `learning`, `cron`, `overlay`, `upstreams`, `updates`, `ui`. An explicit `target:` that names an area is used as is; a module path maps to the first area among its segments (`next_agent_core::router::gateway` → `router`, `coucou_lib::…` → `overlay`); anything else is `app` |
| `event` | A stable snake_case name (see below) |
| `message` | Optional free text, redacted. Present only when the call had a message that is not the event name |
| `fields` | Allowlisted fields of the event |
| `span` | Allowlisted fields of the enclosing spans (outermost first, inner ones win) |
| `dropped_fields` | Present when fields were dropped: how many |

The panic event (`target: app`, `event: panic`) is written by the panic hook itself and carries `crate`, `file` (redacted) and `line`, plus the redacted panic message.

## How to add an event

Use plain `tracing` macros. Crates do not depend on `diagnostics`; only the desktop app does.

```rust
tracing::info!(target: "router", provider = %kind, model = %model, status, duration_ms, "provider_call");
tracing::warn!(target: "channels", status, channel_id = %id, "api_error");
let span = tracing::info_span!("task", task_id = %id); // ids follow a task across events
```

- **Event name:** the message, when it is a snake_case name (`"provider_call"`). To keep a human message too, pass `event = "provider_call"` and a message. Otherwise the event is `unnamed` and the message is kept, redacted.
- **Target:** one of the areas above.
- **Fields:** only names from the allowlist below. Anything else is dropped at runtime and fails `make check`.
- **Strings:** `error` and `reason` hold free text (redacted, cut to 300 characters). Every other string field must look like a token (an id, a model name, a kind): no whitespace, at most 128 characters, or the value becomes `[invalid]`. Numbers and booleans are kept as they are.
- **Never** log a provider or HTTP body, a tool's arguments, or a setting's value. Log the status, the error type or code, the byte count, the tool name and decision, the setting key.
- **Other crates** (hyper, reqwest, tauri …): their events are kept only at `warn` and above.

From the UI, `lib/log.ts` calls `diag_log`; the Rust side applies the same allowlist and redaction (target `ui`; a string `message` field becomes the redacted message; `window` is the calling window's label). The overlay windows log the same way: `routes/coucou/+layout.svelte` connects `lib/log.ts` to `diag_log`.

**Errors from the UI.** A Rust command's error text is written for people and often names their work (a page, board, skill or channel name, a path, git output). So `command_failed` carries `error` only for the commands in `SAFE_ERROR_COMMANDS` (`lib/log.ts`, `commandError`: gateway, usage, upstreams, rules, settings keys, approvals; a new command is not on it until added), `ui_error` in the notes, graph, journals, boards, design, learning, channels, workspace, vault, review and overlay pages sends its `kind` only (`tests/log.test.ts` scans every call), and an unhandled rejection that is a string, not an `Error`, is logged without its text.

## The field allowlist

Kept in one file, `crates/diagnostics/src/fields.rs` (`ALLOWED`); the check script reads it from there.

`error`, `kind`, `code`, `status`, `duration_ms`, `attempt`, `count`, `bytes`, `tokens_in`, `tokens_out`, `tokens_cached`, `cost_micros`, `provider`, `model`, `combo`, `role`, `mode`, `tool`, `decision`, `rule_kind`, `command`, `ok`, `task_id`, `approval_id`, `channel_id`, `channel_kind`, `cron_id`, `session_id`, `window`, `page`, `setting` (the key, never its value), `version`, `from_version`, `to_version`, `migration`, `os`, `arch`, `level`, `reason`, `retries`, `queue`, `files` (a count), `exit_code`, `signal`, `port`.

`message` and `event` are the event's own names. Adding a name needs the coordinator's OK.

**Forbidden names** (`FORBIDDEN`; never allowlisted, a unit test checks the lists do not overlap, and `make check` fails on any log call using one, also as a dotted segment such as `self.prompt`): `prompt`, `content`, `text`, `body`, `answer`, `response`, `message_text`, `title`, `name`, `note`, `query`, `args`, `input`, `output`, `diff`, `path`, `token`, `key`, `secret`, `password`, `cookie`, `authorization`, `sender`, `username`, `email`, `phone`.

## Redaction

`next_agent_diagnostics::redact()` runs on every string written: field values, messages, the panic message, and again on export.

1. **URLs** keep only scheme and host: `https://api.telegram.org/bot…/send` → `https://api.telegram.org/…`.
2. **Secret shapes** become `[secret]`: `sk-…` (including `sk-ant-…`), `AIza…`, `ghp_…`/`gho_…`/`github_pat_…`, `xox…`, Telegram bot tokens, `Bearer …`/`Basic …`, JWTs. `api_key=…`, `token: …`, `password=…` keep the name and hide the value.
3. **Emails** and **phone numbers** become `[redacted]`.
4. **Home folders** (the current one, and any `/Users/x`, `/home/x`, `C:\Users\x`) become `~`.
5. **Paths** shrink to their depth and extension: `~/code/app/src/main.rs` → `~/…/*.rs (depth 4)`, relative `pages/My diary.md` → `…/*.md (depth 2)`. Kept: a one-segment absolute path (`/tmp`) and a two-segment relative one without a file extension (`openai/gpt-4o`, `text/html`). A panic's own source file under `crates/` or `apps/` is kept as is.
6. **Long hex runs** (32+) and **base64-like runs** (40+, mixed case and digits) become `[secret]`.
7. The result is cut to **300 characters**.

Redaction is idempotent, so redacting again on export changes nothing that was already clean.

**What redaction cannot do:** it finds shapes, not meaning. A prompt or note pasted into a free-text field (`error`, `reason`, a message) would pass through as text. That is why provider and HTTP bodies are never logged, why only `error` and `reason` hold free text, and why every other string field must be token-shaped. Wave 2 replaces the existing `%error` calls that may carry provider bodies with a status and an error type.

## Levels

`error`, `warn`, `info` (default) and `debug`. The level is the saved setting `logging.level`, applied once the store opens at startup (the log starts at `info` before that, so `app_started` is always written) and changed at once by `diag_set_level`. No callsite interest is cached, so a change takes effect on the next event.

## Desktop commands (main window only; `diag_log` also in the overlay windows)

| Command | Args | Returns |
|---|---|---|
| `diag_recent` | `{ level?, target?, limit? }`: `level` keeps that level and more severe; `target` one area; `limit` at most 2,000 | `LogEntry[]`, newest first |
| `diag_set_level` | `{ level: "error" \| "warn" \| "info" \| "debug" }` | `void`; saved as `logging.level`, logs `setting_changed` with `setting` |
| `diag_open_folder` | none | `void`; opens the logs folder with the OS opener |
| `diag_export` | none | `{ path, entries }`: `~/Downloads/next-agent-diagnostics-<UTC time>Z.jsonl` |
| `diag_log` | `{ level, event, fields? }` | `void`; at most 20 a second across all windows (the rest are dropped), allowlist and redaction in Rust, `window` set to the caller's label. The only Next Agent command the overlay windows' capabilities grant (`scripts/gen-capabilities.mjs`, `OVERLAY_SHARED`) |

Unknown levels and non-snake_case event names are errors. When the logs folder could not be created at startup, every command answers "the diagnostics log is not available" and the app runs without a log.

## Export

`diag_export` writes one `.jsonl` file: a header line (`export`, `version`, `os`, `arch`, `level`, `exported_at`) and then every line of every log file, oldest first. Each line is parsed and put through the allowlist and redaction again, so a line written by an older build cannot leak through the export; lines that are not log entries are skipped. The owner attaches the file to a bug report themselves.

## Reading the logs

- **In the app:** Settings → Diagnostics (recent entries with level and area filters, open folder, export).
- **In a terminal:**

```sh
cd ~/Library/Application\ Support/dev.nextagent.desktop/logs
tail -f next-agent-$(date -u +%F).jsonl | jq -c '{ts, level, target, event, fields}'
jq -c 'select(.level == "error")' next-agent-*.jsonl                 # all errors
jq -c 'select(.span.task_id == "t_01J…")' next-agent-*.jsonl         # one task
jq -c 'select(.dropped_fields > 0) | {target, event, dropped_fields}' next-agent-*.jsonl
```

## The checks

- **`cargo test -p next-agent-diagnostics`**: redaction, rolling and retention, file modes, ring buffer, runtime level, panic hook, export, and the canary tests: `canary_disallowed_fields_never_reach_the_log`, `canary_secrets_in_allowed_fields_are_redacted`, `canary_ui_events_go_through_the_same_rules`, `canary_export_redacts_again`. They log a fake prompt (`CANARY-PROMPT-7f3a`), a fake `sk-ant-` key, a fake bot token, an email, a note and a home path at `debug`, in allowed and forbidden fields, spans, messages and the UI path, and assert none of them is in the file, the ring buffer or the export.
- **`scripts/check-log-fields.mjs`** (in `make check`): scans `tracing` event and span macros in Rust (`crates/`, `apps/`; not `crates/diagnostics`, whose tests plant forbidden names) and `log.debug|info|warn|error("event", { … })` calls in `apps/desktop/src` and `apps/desktop/coucou/src` (not `lib/log.ts` itself). A forbidden name always fails. A name not on the allowlist fails unless its file is in `scripts/log-fields-allowlist.json`, the pending list of calls that predate the log: one entry per file, `max` = its current count, which may only go down; Wave 2 empties it. In TypeScript the event must be a snake_case literal and the fields an object literal (no spreads or computed keys). `--list` prints every finding. Tests: `scripts/tests/log-fields.test.ts`.
- **`cargo test -p next-agent-desktop --lib log_canary_tests`**: the end-to-end canary tests (below).
- **`scripts/check-boundaries.mjs`** rule 6: no Rust code outside `crates/diagnostics` names the `logs` folder or a `next-agent-*.jsonl` file.

## Privacy rules (summary)

1. Allowlisted fields only; the rest are dropped and counted.
2. Forbidden names can never be allowlisted, and `make check` rejects them in log calls.
3. Every string is redacted and cut to 300 characters.
4. Provider and HTTP error bodies are never logged: status, error type or code, byte count.
5. Tool calls: tool name and decision only, never arguments.
6. Settings: the key only, never the value.
7. Panics: crate, file, line and the redacted message; the file is flushed before the process ends.
8. Canary tests prove it.
9. Nothing leaves the machine: no network sink. Export writes a local file.

## Audit 2026-11-16

Wave 3 of the [[Logging System Plan]]: an end-to-end canary test, a review of every log call (Rust `tracing`, Coucou `log::line`/`fail`, TypeScript `log.*`) against the rules, and a look at a real session's log.

### The canary tests

`apps/desktop/src-tauri/src/log_canary_tests.rs` installs the real subscriber (`next_agent_diagnostics::init`) at `debug` in a temp folder and drives the app's own services with canaries: `CANARY-PROMPT-7f3a` in prompts, notes, names and messages, a fake `sk-ant-api03-…` key, a fake bot token `123456789:AA…` (35 characters), a fake email, a canary page, board, skill and file name, a home path, and a workspace folder named `canary-ws-7f3a`. Each test then reads every log file and the ring buffer, asserts that no canary and no secret shape (`sk-ant-`, `sk-…`, a bot token, an email, `Bearer …`) is there, and that the expected events are (matched on its own task, channel or cron id where the event has one).

| Test | Path | Expected events |
|---|---|---|
| `canary_task_through_the_gateway_with_tools_stays_out_of_the_log` | a manual task through the gateway to a mock OpenAI-compatible provider whose answer calls `write_file`, `memory` and `skill_manage` with canaries, approvals answered, then a final answer echoing them | `task_started`, `provider_call`, `tool_decision`, `approval_requested`, `task_finished`, `memory_saved`, `skill_changed` |
| `canary_provider_error_echoing_the_prompt_stays_out_of_the_log` | the provider answers 400 with a body echoing the prompt and the key | `provider_call_failed` (`status` 400), `task_started`, `task_finished` |
| `canary_channel_message_through_mock_telegram_stays_out_of_the_log` | a mock Bot API: a message from an allowed sender runs a plan-mode channel task and is answered; a stranger gets a pairing code | `channel_started`, `message_in`, `pairing_requested`, `message_out` |
| `canary_notes_page_and_board_saves_stay_out_of_the_log` | a notes page saved through `NotesService`, a file the watcher re-indexes, a board created, saved, saved stale and saved invalid | `graph_indexed`, `graph_reindexed`, `board_saved`, `board_conflict` |
| `canary_memory_editor_and_cron_run_stay_out_of_the_log` | the memory editor (and an over-cap save), a cron job whose name and prompt are canaries, dispatched | `memory_saved`, `cron_fired`, `task_started`, `task_finished` |
| `canary_design_preview_refusals_stay_out_of_the_log` | the Design Studio proxy asked for a canary path with a canary host and a key in a cookie, and a dead dev server | `preview_blocked` (`status` 421, `reason` `wrong_host`), `preview_upstream_down` |

They live in the crate, not `tests/`, because only a test build swaps the OS keychain for memory (`gateway::keys`). The subscriber is global, so the other unit tests of that binary also log into the canary folder; that widens what the check covers. A planted `reason = name` in `boards::save` makes the notes test fail, which shows the check bites. The UI path is covered in `apps/desktop/tests/log.test.ts`, and the diagnostics crate's own canaries are listed under [The checks](#the-checks).

### Leaks found and fixed

| Where | What leaked or was missing | Fix |
|---|---|---|
| `apps/desktop/src/lib/api.ts` (`command_failed`) | Every failed command sent its Rust error text: board, page and skill names (`board "…" not found`), paths with spaces that redaction only half shortens, git output from reviews | `commandError`: the error only for `SAFE_ERROR_COMMANDS`, failing closed |
| `ui_error` in `lib/notes/{store.svelte.ts,PageEditor,GraphView,JournalsFeed}`, `routes/(app)/{boards,design,settings/channels,settings/learning}`, the overlay settings pages, `ConfigReview.svelte`, `lib/app.svelte.ts` (workspace, profiles, undo, tracked tasks, review), Settings (vault) | `String(e)` of those areas' errors, same content as above; overlay errors can carry a provider's reply | `kind` only (the app sites got their own kinds); a test scans every `ui_error` call |
| `lib/log.ts` (unhandled rejections) | A rejected command (a string) sent its text | Only an `Error`'s message is sent |
| `lib/overlay-settings.ts` (`overlay_command_failed`) | `String(err)`: an overlay error can carry a provider's reply | Command name only |
| `crates/core/src/broker.rs` (`step_snapshot_failed`) | `%error` carried git's stderr, which names the workspace's files | `kind = "capture"` only |
| `apps/desktop/coucou/src/core/{flow,bridge,mail}.ts` | `console.error` with the error object, outside the log | `log.warn("overlay_command_failed", { command })`, `overlay_emit_failed` (`kind` = the event name) |
| Coucou windows | No sink: `installGlobalHandlers` ran but nothing was sent | `diag_log` granted to the overlay windows; the layout connects it |

Gaps closed while writing the tests (missing events, not leaks): the agent's `memory` tool wrote no `memory_saved` (`crates/core/src/learning/memory.rs`); replies the hub writes itself (pairing codes, `/stop`, `/new`, media refusals) wrote no `message_out` (`crates/channels/src/hub.rs`); the graph watcher and the preview proxy logged nothing (`crates/graph/src/watch.rs`: `watcher_error` with a notify kind, `graph_reindexed` at `debug`; `crates/design/src/proxy.rs`: `preview_blocked` with status and reason, `preview_upstream_down` and `preview_request_failed` at `debug`).

The review found no other leaks. The remaining free-text `error` fields hold database, keychain, OS, Tauri or network-layer errors, or catalogue and pricing-file parse errors; Coucou's `fail` call sites were audited in Wave 2 ([[Coucou]], "Overlay log audit").

### A real session

The app from the main checkout was already running, so this worktree's build was not started; the UI was checked in the browser preview (Notes, Boards, Settings → Channels and Diagnostics, an overlay page: no console errors) and the backend by the headless tests. Its log (`~/Library/Application Support/dev.nextagent.desktop/logs/`, about 40 minutes, the build before this audit) held no user content. Its `info` was noisy:

- 1,328 of 1,351 lines were `hook_event`, 1,275 of them `PreToolUse`/`PostToolUse`/`PostToolUseFailure` from Claude Code sessions: only session-level kinds (`SessionStart`/`End`, `UserPromptSubmit`, `Stop`, `StopFailure`, `Interrupt`, `SubagentStart`/`Stop`, `Notification`, `PreCompact`) stay `info`; the per-tool-call kinds and any kind not listed are `debug` (`pipe.rs`).
- `db_opened` came three times at every start and once per task: now `debug`; `migration_applied` stays `info`.
- `island_notch` wrote `kind: "[invalid]"` (a struct printed with `?`): now `notch` or `no_notch`.

### Limits

- **Redaction finds shapes, not meaning.** A name or a sentence in a free-text field (`error`, `reason`, a message) passes as text. The defence is not to put such text there: provider bodies, git output and the UI's command errors stay out, and only allowlisted, token-shaped fields carry values.
- **Token-shaped values are not checked for meaning.** A tool or model name the model invents, or a channel kind, is written if it has no spaces.
- **JavaScript exceptions** keep their message (`ui_error` with `kind` `error`): a `JSON.parse` error quotes a few characters of its input.
- **Paths with spaces** are only partly shortened: `not a folder: /Users/x/My Secret Project` becomes `not a folder: ~/My Secret Project`, and `/Users/x/My Notes/a.md` becomes `~/My …/*.md (depth 2)`. Paths are kept out of log calls instead.
- **Not exercised end to end:** the Coucou overlay's Rust paths (relay, dictation, phone link) beyond the Wave 2 audit; the overlay windows' `diag_log` in a running app (covered by the capability test and the shared command only); Windows and Linux builds.
