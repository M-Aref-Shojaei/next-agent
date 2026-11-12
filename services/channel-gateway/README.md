# channel-gateway

OpenClaw-inspired daemon that turns inbound channel events (Telegram, Discord,
Slack, Gmail) into tasks. Channel tasks can never select `bypassPermissions`;
approvals are routed back to the desktop/overlay.

Status: stub (M5). See `docs/plan/Roadmap.md`.
