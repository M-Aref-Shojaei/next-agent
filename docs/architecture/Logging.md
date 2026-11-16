---
title: Logging
tags: [architecture, logging, privacy]
status: implemented (Wave 1)
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

From the UI, `lib/log.ts` calls `diag_log`; the Rust side applies the same allowlist and redaction (target `ui`; a string `message` field becomes the redacted message).

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

## Desktop commands (main window only)

| Command | Args | Returns |
|---|---|---|
| `diag_recent` | `{ level?, target?, limit? }`: `level` keeps that level and more severe; `target` one area; `limit` at most 2,000 | `LogEntry[]`, newest first |
| `diag_set_level` | `{ level: "error" \| "warn" \| "info" \| "debug" }` | `void`; saved as `logging.level`, logs `setting_changed` with `setting` |
| `diag_open_folder` | none | `void`; opens the logs folder with the OS opener |
| `diag_export` | none | `{ path, entries }`: `~/Downloads/next-agent-diagnostics-<UTC time>Z.jsonl` |
| `diag_log` | `{ level, event, fields? }` | `void`; at most 20 a second (the rest are dropped), allowlist and redaction in Rust |

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
