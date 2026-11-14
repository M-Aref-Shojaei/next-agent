---
title: OpenClaw
tags: [upstream]
status: researched
---

# OpenClaw — what we port

- Repo: https://github.com/openclaw/openclaw
- Licence: **MIT** © 2026 OpenClaw Foundation
- Built with TypeScript on Node 24 and pnpm.
- We run channels inside the app as a background service, in Rust. We skip OpenClaw's WebSocket gateway, its device/canvas nodes and its plugin loader.

## Our rules come first (stricter than OpenClaw)
- Tasks from channels can **never** use bypass mode.
- Channel tasks get a deny-by-default tool set. The tools for gateway control, cron, spawning sessions and sending to other sessions are always denied.
- **Approvals only happen on the desktop and in the overlay.** We do not copy OpenClaw's approve buttons inside chats.
- If no desktop UI is reachable, or an approval times out, the answer is deny (OpenClaw's `askFallback=deny`).
- Content arriving from a channel is untrusted and is wrapped as untrusted before it reaches the agent.

## Ranked parts to port (sizes are Rust estimates)

1. **DM pairing and allow-lists** (`src/pairing`, `src/security`, ~500–800 lines)
   - `dmPolicy` takes one of four values:

     | Value | Meaning |
     |---|---|
     | `pairing` | Default. Unknown senders must pair first. |
     | `allowlist` | Only listed senders. |
     | `open` | Anyone. Needs an explicit `"*"`. |
     | `disabled` | No DMs. |

   - A pairing code expires after 1 hour.
   - Re-DMing does not issue a new code.
   - At most 3 pairing requests can be pending per channel or account.
   - The owner approves a pairing **in the desktop app**.
   - Approvals are stored per channel and account in SQLite.

2. **Session routing** (`src/routing`, `src/sessions`, ~600–1,000 lines)
   - This is a pure function: (channel, account, peer, group, thread) → session key.
   - `dmScope` decides how DMs map to sessions. **Our default is `per-channel-peer`.**

     | Value | Meaning |
     |---|---|
     | `main` | All DMs share one session. |
     | `per-peer` | One session per person. |
     | `per-channel-peer` | One session per person per channel. |
     | `per-account-channel-peer` | Also split by bot account. |

   - `identityLinks` merges one person's identities across channels.
   - Groups get one session each.
   - Requests within a session run in order. Different sessions run concurrently.
   - `/stop` cancels the current turn.
   - Reset policies: none, daily at 04:00, after idle, or manual `/new`.

3. **Channel trait and supervisor** (~400 lines)
   - Trait methods: `config`, `start(inbound_tx)`, `send(outbound)`, `pairing`, `threading`, plus capability flags.
   - The supervisor reconnects channels and reports their health.

4. **Group gating** (~300 lines)
   - Checks run in this order: group policy → sender allow-list → mention or reply activation.
   - Replying to a bot message does **not** authorise the sender.

5. **Outbound pipeline** (~600–1,000 lines)
   - Long text is split per channel: Telegram has a `textChunkLimit`; Discord allows 2,000 characters and 17 lines.
   - Media size caps: WhatsApp 50 MB, Discord 100 MB.
   - Streaming previews work by editing the message.
   - Idempotency keys stop duplicate sends.

6. **Scheduler** (~500 lines)
   - Deadlines are saved to disk.
   - One timer drives everything.
   - Ticks missed while the computer slept are merged into one.
   - Results go to a channel. Merge this with the Hermes cron work (see [[Hermes Agent]]).

## Channels

| Channel | Upstream library | Auth | Rust option | Effort |
|---|---|---|---|---|
| Telegram | grammY, long polling | Bot token | `teloxide` or raw `reqwest` | 3–5 days. **First.** |
| Discord | Gateway WebSocket | Bot token, privileged intents | `serenity` or `twilight` | 5–8 days |
| Slack | Bolt, Socket Mode | Bot token + app token | `slack-morphism` | 5–7 days |
| Gmail | Pub/Sub watch | Google OAuth | `google-gmail1` or REST + `oauth2` | 5–8 days |
| Matrix | — | — | `matrix-sdk` | Medium |
| WhatsApp | Baileys (unofficial) | QR code | Official Cloud API only | High risk. **Skip** the unofficial protocol. |
| Signal, iMessage | External helper programs | — | — | Later, maybe |
