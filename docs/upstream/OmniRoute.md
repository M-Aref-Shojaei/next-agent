---
title: OmniRoute
tags: [upstream]
status: researched
---

# OmniRoute — what we add to the gateway

- **Repo:** https://github.com/diegosouzapw/OmniRoute
- **Licence:** **MIT** © 2026 diegosouzapw.
- **Studied:** v3.8.53 (2026-11-06). Branch `release/v3.8.54`.
- **Origin:** a **fork of [[9Router]]**, ported to TypeScript. It also draws on CLIProxyAPI (Go) and LiteLLM pricing data.
- **What we do with it:** we only port ideas, in Rust, inside `crates/core/src/router/`. File headers read `// Ported from OmniRoute (MIT, © 2026 diegosouzapw): <path> @ v3.8.53`.

## Decision (2026-11-14, coordinator, delegated by owner)

**Building:**
- exact-match cache
- circuit breaker
- prompt-cache affinity
- orderings: `weighted`, `cost-optimized`, `lkgp`, `context-optimized`
- budget caps

**Skipped, and why:**
- `p2c`, `least-used` and `strict-random`: these balance load across many keys, so they don't help a single user.
- semantic cache: needs an embedder. Revisit after built-in local models exist.
- auto-combo: the smart strategy covers it.
- PII masking: a single user's own data stays local.
- batch, extra compression engines, fusion.
- all free-access tricks.

## Add (research)

1. **Exact-match response cache, plus an optional semantic layer.**
   - **Exact key:** SHA-256 of canonical JSON containing the schema version, model, provider, normalised messages, `temperature` (default 0), `top_p` (default 1), and the output contract (tools, response_format).
   - **Semantic layer:** embeds the last 3 messages and finds the nearest cached entry by cosine similarity, with k=1. It is scoped by model and provider. Requests that use tools or a response format are excluded.
   - **When it is skipped:** `no-cache`, or a non-zero temperature (configurable).
   - **What it stores:** the response, including stream chunks, which are replayed as SSE. Each entry has a TTL.
   - **Failure:** if the cache errors, the request goes through as normal.
   - **For us:** store it in SQLite, with embeddings as f32 BLOBs and a linear cosine scan. The embedder is a local Ollama, or any OpenAI-compatible `/v1/embeddings` endpoint. The exact layer ships first; the semantic layer is opt-in.
   - **Source:** `open-sse/services/cache/*`
2. **Per-provider circuit breaker.**
   - Three states: closed → open → half-open.
   - It opens after 3 consecutive failures, and the next probe comes 60 s later.
   - Any success closes it. A failure during half-open reopens it.
   - **Improvements over upstream:** allow only one probe in flight at a time, and grow the cooldown on repeated trips.
   - Breaker state feeds the routing score: closed 1.0, half-open 0.5, open 0.
   - **Source:** `src/lib/resilience/adaptiveCircuit.ts`
3. **Prompt-cache affinity.**
   - **Key:** the `prompt_cache_key`, or a hash of the stable message prefix.
   - **Score per target:** `0.75·hash(key‖target) + 0.25·availability`, sorted from highest to lowest.
   - **Effect:** the same conversation keeps going to the same account, so provider prompt caches stay warm.
   - **Source:** `open-sse/services/combo/promptCacheAffinity.ts`
4. **More combo orderings** (`open-sse/services/combo/targetSorters.ts`).

   | Ordering | How it picks |
   |---|---|
   | `weighted` | Random draw over each target's weight |
   | `p2c` | Picks two at random and keeps the better: success rate + 1/log10(latency+10) − penalty |
   | `least-used` | Fewest requests so far |
   | `cost-optimized` | Lowest input price |
   | `strict-random` | Shuffled deck, so each target is used once per cycle |
   | `lkgp` | Last known good provider first |
   | `context-optimized` | Largest context window first |

## Maybe

- **Budget caps.** A per-day or per-month spend limit per role, built on our cost tracking.
- **Cheap auto-combo.**
  - Ranks targets by cost and latency alone.
  - Has mode packs such as cost-saver and ship-fast.
  - A budget header either returns HTTP 402, or falls back to the cheapest option.
  - The full 16-factor version needs more telemetry than we collect.
- **PII masking.** Redact or detect-only modes, using regex, on requests. A prompt-injection guard is weak upstream.
- **Extra compression engines.** Session-dedup back-references, and columnar compaction for JSON arrays.
- **Batch API.** JSONL input, a checkpoint per item, retries with jitter.

## Skip

- **Fusion.** Already deferred.
- **Quota-share, subscription ladder, multi-tenant features.** Not needed for a single-user app.
- **MCP server (110 tools), A2A, a 67-language UI.** Out of scope.
- **Risky free-access features. Never port these:**
  - keyless and web-session providers;
  - browser-backed chat with CAPTCHA/Turnstile solving;
  - TLS and fingerprint spoofing;
  - pooling consumer OAuth accounts (Claude, Codex);
  - free-tier chaining.

  They break provider terms and can get accounts banned.
