---
title: OpenRouter
tags: [upstream]
status: researched
---

# OpenRouter — ideas we add to the gateway

- **Source:** the docs at https://openrouter.ai/docs and the GitHub org https://github.com/OpenRouterTeam.
- **What's open:** the production router is **closed source**, so there is no gateway code to port. Only the SDKs are open (Apache-2.0); `tool-bench` is MIT (Rust).
- **How we use it:** we re-implement ideas from the docs and credit OpenRouter in [[9Router]]-style headers ("Design adapted from OpenRouter docs: <page>"). We copy no code.
- **Terms of service:** they bar scraping and reselling. For the models catalog we only call the public endpoint when the user asks to refresh, cache the result locally, and never redistribute or bundle it.

## Added to the gateway (Wave 1 session 1, before merge)

1. **Models catalog import.**
   - Source: `GET https://openrouter.ai/api/v1/models` (auth optional).
   - Fields per model: `id`, `canonical_slug`, `context_length`, `architecture.{input,output}_modalities`, `supported_parameters`, `top_provider.max_completion_tokens`.
   - Pricing is per token for `prompt`, `completion`, `input_cache_read`, `input_cache_write` and `internal_reasoning`. A price of `"-1"` means dynamic and `"0"` means free. Skip `pricing.overrides` whose conditions we don't recognise.
   - Use: fills our pricing table, context limits and capabilities. Match models by slug or prefix.
   - Runs only on a manual "Refresh" in Settings.
2. **Cache-aware cost.**
   - Read `cached_tokens` and `cache_write_tokens` from usage.
   - Apply per-provider multipliers to the input price:

     | Provider | Cache read | Cache write |
     |---|---|---|
     | Anthropic | 0.1× | 1.25× (5-min TTL) or 2× (1 h) |
     | DeepSeek | 0.1× | — |
     | OpenAI / Grok | ~0.25–0.5× | — |
     | Gemini | 0.25× | — |

   - Show the cache hit rate on the Usage page.
3. **Unified `reasoning` setting.**
   - Request shape: `{effort | max_tokens, exclude, enabled}`.
   - Effort as a share of the output limit: xhigh/max 95%, high 80%, medium 50%, low 20%, minimal 10%.
   - Per-provider mapping:
     - Anthropic: budget = `max(min(max_output·ratio, 128000), 1024)`, and `max_tokens` must be greater than the budget.
     - OpenAI: `effort` passes through.
     - Gemini 3: `thinkingLevel`, with xhigh mapped to high.
     - Gemini 2.5: `thinkingBudget`.
   - Responses expose `reasoning` and `reasoning_details[]` (summary, text or encrypted). **Pass these back unchanged during tool use.** When streaming, concatenate the deltas in order.
4. **Context overflow handling ("middle-out").**
   - When a prompt exceeds the model's `context_length`, first try another model in the combo that has enough context.
   - Otherwise trim from the middle: keep the start and the end, and trim the middle tool outputs first.
   - This is a cheap safety net. It runs before the agent engine's full compaction.
5. **Provider preferences.**
   - Combos gain `sort` (price, latency or throughput) and `max_price`.
   - `sort` reorders models using recorded cost, p50 latency and tokens per second.
   - `max_price` excludes models over the limit, and the request fails if none are left.
   - Presets: `:floor` = cheapest first, `:nitro` = fastest first.

## Maybe later
- `session_id` sticky routing (our smart combo mostly covers this).
- Weighted load balancing by throughput, time to first token, and 429 or error rate.
- An Auto Router heuristic.
- Repairing invalid structured-output JSON.
- A web search tool with citations.
- Local PDF text extraction.

## Skipped
- The `models` fallback array (combos already do this).
- `:free`, `:online` and `:thinking` variants.
- Presets (model roles and combos cover this).
- BYOK (every provider in our gateway is already bring-your-own-key).
- ZDR, data-collection and quantization filters (these only matter when routing through OpenRouter itself).
