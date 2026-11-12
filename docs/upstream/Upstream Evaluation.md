---
title: Upstream Evaluation
tags: [upstream]
status: todo
---

# Upstream Evaluation

To complete in M0. For each upstream record: licence, language/runtime, maturity, what we reuse, integration mode (**reference** = read for patterns, **adapter** = run unmodified behind an interface, **fork** = vendored and patched), and risks. Fields marked *verify* are not yet confirmed.

| Upstream | Reuse | Runtime | Integration mode (proposed) | Licence | Milestone |
|---|---|---|---|---|---|
| `anomalyco/opencode` | Coding-agent engine, tool & session patterns | TS (Bun) — *verify* | Adapter via `services/agent-gateway`; tools delegated to broker | *verify* | M2 |
| `decolua/9router` | Local OpenAI-compatible router on :20128, provider mgmt, RTK token optimization | *verify* | External service (user-run), later managed sidecar | *verify* | M1 (probe), later |
| `openclaw/openclaw` | Channel gateway & multi-channel automation architecture | TS — *verify* | Reference → own `services/channel-gateway` | *verify* | M5 |
| `NousResearch/hermes-agent` | Python runtime, skills/memory `state.db`, subagents | Python | Adapter via `services/hermes-adapter` (`uv`) | *verify* | M2 (opt.) |
| `Louis-CFM/coucou` | Notch / Dynamic-Island status & approval overlay patterns | *verify* | Reference only | *verify* | M3 |
| Logseq | Outliner, block refs, journals, graph concepts | Clojure | Concepts only; file format compatibility | AGPL-3.0 — *verify* | M4 |

## Checklist per upstream
- [ ] Licence and its effect on our distribution (copyleft? attribution?)
- [ ] Can tool execution be delegated to an external broker?
- [ ] Headless / programmatic API (stdio, HTTP, SDK)?
- [ ] Runtime requirements and size when bundled
- [ ] Release cadence & pinning strategy
- [ ] Security posture (telemetry, auto-update, network calls)

Create one note per upstream in this folder as evaluations complete (e.g. `opencode.md`).
