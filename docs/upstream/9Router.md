---
title: 9Router
tags: [upstream]
status: researched
---

# 9Router — what we port

- Repo: https://github.com/decolua/9router · **MIT** © 2024-2026 decolua and contributors · studied v0.5.119, commit `1855aa4` (2026-11-12).
- Node/Next.js app. The provider-agnostic engine is `open-sse/`. That's the part we port to Rust. We don't bundle 9Router itself ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]).
- Line counts are estimates (±10%). Anything marked *verify* was not read in full.

## Request pipeline (mirror this order)
1. Parse the request and resolve the model or combo.
2. Translate it to the provider's format.
3. Run the token savers: **RTK → (Headroom, skipped) → Caveman/Ponytail → custom prompt**.
4. Call the provider. On 401/403, refresh the token and retry once.
5. Translate the response back and stream SSE.

Savers only run when `tokenSaverEnabled` is on, and the header `X-9Router-Token-Saver: off` turns them all off. Every saver is **fail-open**: on any error the request passes through unchanged.

## 1. Caveman / Ponytail / custom prompt (`open-sse/rtk/caveman.js`, `cavemanPrompts.js`, `systemInject.js`, `ponytail*.js`, `customPrompt.js`)
- A system-prompt injector that tells the model to answer tersely (Caveman) or to write minimal code (Ponytail, "YAGNI"). The README claims up to 65% fewer output tokens.
- Caveman levels: `lite | full (default) | ultra | wenyan-lite | wenyan | wenyan-ultra`. Ponytail levels: `lite | full | ultra`.
- Each prompt is 2–3 level-specific instructions plus shared rules:
  - leave code and commands untouched;
  - switch back to normal text when clarity needs it;
  - keep the style across turns;
  - allowed abbreviations;
  - answer in the user's language;
  - no self-reference, no decoration.
- **Write our own prompt text.** The exact upstream wording was not captured.
- Runs after translation. Injection per format:
  - OpenAI: append to the first `system`/`developer` message, or insert a system message at index 0.
  - Responses API: append to `instructions` (when it is a string) or to the `input[]` system item.
  - Anthropic: append to `system`. If `system` is an array, insert the text block *before* the last block that has `cache_control`, so the cached prefix stays valid.
  - Gemini: push a part into `systemInstruction`.
- Idempotent: if the prompt is already present as a whole `\n\n`-separated segment, it is not added again.
- Rust estimate: ~200 lines.

## 2. RTK — tool-output compression (`open-sse/rtk/index.js`, `autodetect.js`, `constants.js`, `filters/*`)
- Compresses **only tool results**:
  - OpenAI `role:"tool"` messages;
  - Responses `function_call_output`;
  - Anthropic `tool_result`.
- Skips a result when:
  - it is an error result;
  - it is shorter than 500 characters;
  - it is longer than 10 MiB.
- Keeps the compressed output only if it is non-empty and strictly shorter than the original.
- Detection looks at the first 1024 characters. The first matching detector wins, in this order:
  1. git log
  2. git diff
  3. git status
  4. build output
  5. porcelain status (at least 3 lines, at least 60% matching)
  6. grep (`file:N:` lines)
  7. find (path-like lines)
  8. tree (box-drawing characters)
  9. ls (`total N`, or at least 3 permission strings)
  10. search-list
  11. read-numbered (at least 250 lines, at least 70% matching `N|`)
  12. dedup-log (at least 5 non-empty lines)
  13. smart-truncate (at least 250 lines)
- Limits:

  | Output type | Limit |
  |---|---|
  | Diff hunk | 100 lines, 3 context lines |
  | git log | 200 lines |
  | grep | 10 matches per file |
  | find | 10 files per directory, 20 directories |
  | git status | 10 modified + 10 untracked |
  | ls | top 5 extensions; drops `node_modules`, `.git`, `target`, `dist` |
  | tree | 200 lines |
  | smart-truncate | first 120 + last 60 lines |

- Records `bytesBefore`, `bytesAfter` and per-filter hits. The README claims 20–40% input savings.
- The filter bodies were not read (*verify*), so design the output formats ourselves.
- Rust estimate: 800–1,200 lines.
- **Prompt caching (copy this):** for Anthropic requests, add `cache_control: {type:"ephemeral"}` to the system text, to the last tool, and to the last block of the last assistant message.

## 3. Combos and fallback (`open-sse/services/combo.js` ~770 lines, `accountFallback.js` ~240 lines)
- A combo is `{id, name UNIQUE, kind:"llm", models: ["provider/model", …]}`.
- Strategy is resolved in this order: per-combo setting, global setting, then `fallback`.
- Strategies:
  - `fallback`: try the models in order.
  - `round-robin`: rotate, advancing after `stickyLimit` uses (default 1).
  - `smart`: stick to one model per session; switch based on a confidence threshold (0.7, 0.8 or 0.9).
  - `fusion`: send to every panel model in parallel (no streaming, no tools); a judge model writes the answer. Minimum panel 2, 8 s grace after quorum, 90 s hard timeout.
- **Capability reorder:** look at the last user turn for vision, pdf, audio or video input. Stable-sort models so fully capable ones come first, hard-capable ones next, the rest last. Nothing is dropped.
- Per-model attempt, made once:
  - **2xx SSE:** peek at the stream. Empty content means try the next model, unless this is the last model or the finish reason says not to retry.
  - **Non-2xx, request-scoped** (4xx other than 401/402/403/429): remember the first such response and move to the next model.
  - **502/503/504** with a cooldown of 5 s or less: sleep, then move to the next model.
  - **Thrown error:** move to the next model.
  - **All failed:** return the first request-scoped response, otherwise 503.
- **Account cooldown:**
  - Formula: `min(base·2^(level−1), max)`, which gives roughly 1 s, 2 s, 4 s … up to 4 min (*verify* the constants).
  - Per-account state: `rateLimitedUntil`, `backoffLevel`, `lastError`, and per-model locks.
  - Accounts are picked by `priority` ascending, skipping any in an exclude set.

## 4. Providers and translation (`open-sse/translator/{request,response}/*`, `providers/registry/*`)
- Provider types:
  - API-key providers (40+): OpenAI, Anthropic, Gemini, OpenRouter, DeepSeek, Groq, xAI, Mistral, Together, Fireworks, Cerebras, NVIDIA, GLM, Kimi, and others.
  - OAuth providers.
  - Vertex (service account).
  - Custom OpenAI-compatible endpoints.
- A connection is `{id, provider, authType, name, priority, isActive, data:{apiKey|accessToken, refreshToken, expiresAt, …}}`.
  - Upstream stores these as plain JSON. **We keep secrets in the OS keychain.**
- Token refresh:
  - starts 5 minutes before expiry;
  - up to 3 tries, waiting `attempt × 1 s`;
  - stops on `invalid_grant` and other unrecoverable codes.
- Translation pivots through the OpenAI format.
- **OpenAI → Anthropic:**
  - `system` messages become the `system` array.
  - Roles: `user` and `tool` become `user`; everything else becomes `assistant`. Adjacent turns from the same role are merged.
  - Each `tool_result` goes in its own user message right after its `tool_use`.
  - Tools become `{name, description, input_schema}`.
  - Images: a data URI becomes a base64 source; an http(s) URL becomes a url source; a PDF becomes a `document`.
  - `tool_choice`: `required` → `any`; a named function → `tool`; everything else → `auto`.
  - `max_tokens` is capped at the model's ceiling.
- **OpenAI → Gemini:**
  - Messages become `contents` with roles `user` and `model`.
  - Tool calls become `functionCall`; results become `functionResponse`.
  - Duplicate call IDs get a `-2` suffix.
  - `systemInstruction` holds the system prompt; `maxOutputTokens` replaces `max_tokens`.
  - Function names are sanitised to at most 64 characters.
- Skip: Kiro, Cursor (protobuf), Ollama variants, Headroom, pxpipe.
- Rust estimate: 1,500–2,500 lines.

## 5. Usage and cost (`src/lib/db/repos/usageRepo.js` ~815, `open-sse/providers/pricing.js` ~500)
- One row per request: `timestamp, provider, model, connectionId, endpoint, status, promptTokens, completionTokens, cachedTokens, reasoningTokens, cost`.
- Daily rollups: `requests`, token and cost totals, and breakdowns `byProvider`, `byModel`, `byAccount`, `byEndpoint`.
- Cost in USD, prices per 1M tokens:
  `(max(0, input−cached−cacheCreation)·in + cached·(cachedPrice||in) + output·out + reasoning·(reasoningPrice||out) + cacheCreation·(cacheCreationPrice||in)) / 1e6`
- Price lookup order: provider override, free namespaces, model table (vendor prefix stripped), glob patterns. Unknown models cost 0, and every cost is labelled an estimate.
- Dashboard:
  - periods: 24h, today, 7d, 30d, all;
  - totals and breakdowns;
  - hourly or daily chart;
  - the 20 most recent requests.
- Rust estimate: 400–700 lines, plus a price data file.

## Port status (Wave 1, LLM gateway)
Everything lives in `crates/core/src/router/` behind `CompletionClient` and `HealthProbe`.

| 9Router part | Our file | Notes |
|---|---|---|
| Providers, connections | `providers.rs` | Presets: OpenAI, Anthropic, Gemini, OpenRouter, DeepSeek, Groq, xAI, Mistral, Together, Fireworks, Ollama, LM Studio, 9Router, custom. Keys in the OS keychain (`provider:<id>`), rows in `llm_connections`. |
| Translation | `translate/{mod,anthropic,gemini}.rs` | OpenAI is the pivot. Requests OpenAI → Anthropic/Gemini; responses and SSE streams back to OpenAI. |
| Combos, fallback, cooldown | `combo.rs`, `gateway/mod.rs` | `fallback` and `round-robin`; capability reorder; fall-through rules; backoff 1 s … 240 s. |
| Caveman / Ponytail | `savers/prompts.rs` | Our own prompt text; levels lite/full/ultra; idempotent per format. |
| RTK | `savers/rtk.rs` | 13 detectors in 9Router's order and limits; our own output formats. |
| Prompt caching | `savers/mod.rs` | `cache_control` on system, last tool, last assistant block. |
| Usage, cost | `pricing.rs`, `pricing.json`, `store.rs` (`llm_usage`, `llm_usage_daily`) | Unknown models cost 0 and are flagged. A `pricing.json` in the app data folder overrides entries. |

Not ported: `smart` and `fusion` strategies, OAuth providers and token refresh, the Responses API format, Wenyan Caveman levels, the custom-prompt saver, Headroom, inbound Anthropic/Gemini-format requests (we have no HTTP server; the agent always speaks OpenAI format).
