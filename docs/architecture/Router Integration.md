---
title: Router Integration
tags: [architecture, llm]
status: draft
---

# Router Integration

Next Agent talks to an **OpenAI-compatible** endpoint served by 9Router (or compatible), default `http://127.0.0.1:20128/v1`.

## M1 scope
- Settings: base URL, API key (stored in OS keychain), default model, per-task model override.
- Health: `GET {base}/models` every 15 s (and on demand), 3 s timeout → `ok` (200 + models), `degraded` (slow / auth error), `down` (connect refused/timeout). Surfaced in the status bar.
- Calls: `POST {base}/chat/completions` with `stream: true`, tools/function calling.
- Errors: retries with backoff for 429/5xx; clear UI error for 401 / missing model.
- Privacy: requests logged as metadata (model, tokens, latency) — prompts stored only as task events.

## Later
- Optional managed 9Router sidecar via [[Process Manager]] (Q2 in [[Risks and Open Questions]]).
- Provider management UI (`services/router-adapter`) reusing 9Router's config API; token-optimization (RTK) patterns surfaced as stats.

Verify 9Router's actual API surface/port/licence during [[Upstream Evaluation]].
