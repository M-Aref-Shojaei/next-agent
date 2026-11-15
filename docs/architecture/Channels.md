---
title: Channels
tags: [architecture, security]
status: implemented (Telegram)
---

# Channels

Chat channels are another way to start tasks. A message to the bot becomes a task in the normal pipeline (`spawn_task` → broker → permission engine), and the task's final answer goes back to the chat. Design ported from [[OpenClaw]] at commit `1ac07b3` (MIT); our rules are stricter.

Code: `crates/channels` (no Tauri, fully testable) and `apps/desktop/src-tauri/src/channels.rs` (the app wiring and commands). UI: Settings → Channels (`/settings/channels`).

## Rules (stricter than OpenClaw)

| Rule | Where it is enforced |
|---|---|
| Channel tasks run only in `manual` or `plan`; never `auto`, never bypass | `ChannelMode` has no other variant (config JSON with another mode does not parse); `config::guard_mode` before every spawn (`AppService::task_start_with`); the engine refuses bypass for `Origin::Channel` |
| Deny-by-default tool set; the channel profile only tightens | `profile::channel_profile`: `*: deny`, the configured tools `allow`, control-plane tools always `deny` (`subagent`, cron, session/channel send, gateway, `ask_user`). It is an ancestor in the built-in agent's profile chain (denied tools are not offered) **and** wraps every executor (`ChannelExecutor`), so each call carries its floor. An allowed tool still goes through the permission engine |
| Inbound content is untrusted | `untrusted::task_prompt` wraps the message (and replayed history) between random-id boundaries; spoofed markers and model special tokens are neutralised |
| Approvals only on the desktop or overlay | No approval buttons or commands in chat. The chat only says "waiting for approval on the desktop…". The approval card is the normal one (`approval_respond`) |
| Saved rules never approve for a chat | `TaskOptions::untrusted_origin` → `SqliteBroker::untrusted_origin`: stored **allow** rules ("Always for this workspace") are not loaded for channel tasks, only deny rules. The card offers only Allow and Deny; an "always" answer from an older client counts as allow-once and saves no rule |
| Secrets stay behind a prompt | For channel tasks the broker turns an allowed read or write of a secret or repository-control file (`.env*`, `.envrc`, `.git/`, `.next-agent/`) into an approval, judged on the resolved path so a link cannot dodge it |
| No answer = deny | `TaskOptions::approval_timeout` (per channel, default 300 s, 10–3600 s): `ChannelApprover` resolves `deny` `by: "timeout"`. If no window is open, nobody answers, so it times out |
| Bot tokens in the OS keychain | `secret::token_entry` (`dev.nextagent.desktop` / `channel:<id>`). Never in SQLite, never sent back to the UI (only `hasToken`), scrubbed from every error, redacted in `Debug` |

## Flow

1. **Adapter** (`Channel` trait: config, capabilities, probe, start(inbound), send, edit, typing, threading, pairing label). Telegram: long polling `getUpdates` (25 s), plain-text `sendMessage` chunked at 4,096 UTF-16 units, `editMessageText` for previews, `sendChatAction` typing. Media is not supported yet: the bot replies "Media is not supported yet".
2. **Supervisor** restarts the adapter after transient errors (1 s doubling to 60 s, honouring `retry_after`), stops on fatal ones (rejected token) and publishes health: `starting`, `connected`, `reconnecting`, `failed`, `stopped`.
3. **Hub** (`hub.rs`), per message: inbound idempotency key → access → commands → routing → per-session queue → task → reply.
   - **DM policy**: `pairing` (default), `allowlist`, `open` (needs an explicit `*` on the allow-list), `disabled`.
   - **Pairing**: unknown senders get an 8-character code; it expires after 1 h; at most 3 requests wait per channel and further ones are refused (not evicted). The bot answers only when it issues a code: repeats and a full queue stay silent. In Settings → Channels the code is not shown: the owner types the code the person reads in their chat to approve it (Deny works per row). Approving adds the sender to the allow-list and tells them. Display names without a username never start with `@`.
   - **Groups**: group policy (`disabled` default, `allowlist`, `open`) → sender allow-list, by id (the `*` that opens DMs does not open groups) → activation (mention or reply to the bot, if `requireMention`). A reply to the bot never authorises the sender. Groups never issue pairing codes.
   - **Routing** (pure): `dmScope` `per-channel-peer` by default (`main`, `per-peer`, `per-account-channel-peer`), `identityLinks` merge one person across channels, each group (and forum topic) has its own session.
   - **Queue**: one ordered lane per session, sessions run concurrently. `/stop` cancels the running task (its pending approvals resolve as deny) and drops queued ones; `/new` also forgets the session's history. The last 6 turns are replayed into the next prompt, each question with its own sender and each earlier answer wrapped as untrusted too; history lives in memory only.
   - **Outbound**: chunks keyed `out:<channel>:<chat>:<message>:<part>`, so a redelivered message is not answered twice; streaming previews edit one message (at most every 1.5 s) and the final answer replaces it.
4. **Desktop runner** calls `AppService::task_start_with(…, Some(ChannelLaunch))` in the channel's workspace, emits `task-started` with an `origin` (e.g. `Telegram · @alice`) and mirrors the task stream to every window, so the task, its origin notice and its approvals show in the main timeline and the overlay.

## Storage (`crates/core/src/store.rs`, `// --- channels ---`)

`channels` (non-secret config JSON), `channel_pairings`, `channel_allow`, `channel_idempotency` (pruned after 30 days), `channel_activity` (last 2,000 per channel).

## Scheduler hook: deliver to a channel

`ChannelService::deliver(&Delivery { channel_id, chat_id, text, idempotency_key })` (wrapping `Hub::deliver`) sends a message outside a conversation. It only reaches chats that may talk to the bot under the current policies (an allow-listed sender's DM while DMs are on, or a listed group while groups are on), chunks like replies, and with the same `idempotency_key` sends once: each chunk's key is claimed before sending (so concurrent retries cannot both send) and released if the send fails.

Integration with the memory-skills cron (to wire after both branches merge):

1. The cron job gets an optional target `{ "channel": "<channel id>", "chat": "<chat id>" }`.
2. When a job with a target finishes, the scheduler calls `channels.deliver(&Delivery { …, idempotency_key: Some(format!("{job_id}:{scheduled_at}")) })` with the job's result text, where `channels` is the `Arc<ChannelService>` managed by Tauri (`app.state::<Arc<ChannelService>>()`).
3. Jobs started *from* a channel are not possible: `schedule_job`, `list_jobs` and `cancel_job` are on the channel profile's always-denied list.

## Owner setup (Telegram)

1. In Telegram, talk to **@BotFather**: `/newbot`, pick a name and a username ending in `bot`. Copy the token it gives you (`123456:ABC…`).
2. Optional: for groups, `/setprivacy` → Disable lets the bot see mentions in groups (with privacy on it only sees commands and replies).
3. In Next Agent: Settings → Channels → Add Telegram. Paste the token, pick the workspace, the mode (manual or plan), the DM policy and the tools. **Test connection** shows the bot's username. Save: the token goes to the OS keychain and the channel starts; its health turns `connected`.
4. Message the bot from your Telegram account. It replies with a pairing code. Approve the code under **Pending pairings** in Settings → Channels.
5. Message again: the task appears in the main window (marked with its origin) and in the overlay. Approve or deny writes and commands there. The answer arrives in Telegram.

If the bot already has a webhook (from another tool), long polling fails with 409; remove the webhook (`deleteWebhook`) or stop the other program.

## Not yet

Media in and out; reset policies other than `/new` (daily, idle); persistent conversation history; per-sender tool profiles; Discord, Slack and Gmail (Wave 3).
