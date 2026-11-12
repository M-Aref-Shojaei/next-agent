---
title: Router Integration
tags: [architecture, llm]
status: draft
---

# Router Integration

Next Agent talks to an **OpenAI-compatible** endpoint served by 9Router (or compatible), default `http://127.0.0.1:20128/v1`.

## M1 scope
- Settings: base URL, API key (stored in OS keychain), default model, per-task model override.
- Health: `GET {base}/models` every 2 s by default (M1 acceptance requires the status dot to change within 5 s; the earlier 15 s figure could not meet that — interval is configurable via `MonitorConfig`) and on demand, 3 s timeout → `ok` (200 + models), `degraded` (slow / auth error), `down` (connect refused/timeout). Surfaced in the status bar.
- Calls: `POST {base}/chat/completions` with `stream: true`, tools/function calling.
- Errors: retries with backoff for 429/5xx; clear UI error for 401 / missing model.
- Privacy: requests logged as metadata (model, tokens, latency) — prompts stored only as task events.

## As built (M1)
- `crates/core/src/router/`: `RouterSettings` (settings keys `router.base_url`, `router.default_model`), `KeyringSecret` (service `dev.nextagent.desktop`, account `router-api-key`), `RouterClient` (`health`, `list_models`, streaming `chat`; implements `CompletionClient` and `HealthProbe`), `ReqwestFetcher` (`http_fetch` tool transport).
- `crates/process-manager`: `HealthMonitor::spawn(probe, MonitorConfig)` publishes `HealthReport { status: ok|degraded|down, detail, latency_ms, checks }` on a `watch` channel; `refresh()` probes immediately.
- Health mapping: 200 + ≥1 model under 1.5 s → `ok`; slow / 401 / non-2xx / empty or malformed list → `degraded`; connect failure / timeout → `down`.
- Chat: 429/5xx/connect errors retried (3 attempts, exponential backoff from 500 ms) before the stream opens; 401 → `Unauthorized`, 404 or "model not found" → `ModelNotFound`. Non-streaming JSON answers are accepted too.

## Later
- Optional managed 9Router sidecar via [[Process Manager]] (Q2 in [[Risks and Open Questions]]).
- Provider management UI (`services/router-adapter`) reusing 9Router's config API; token-optimization (RTK) patterns surfaced as stats.

Verify 9Router's actual API surface/port/licence during [[Upstream Evaluation]].
