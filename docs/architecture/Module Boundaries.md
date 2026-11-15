---
title: Module Boundaries
tags: [architecture]
status: draft
---

# Module Boundaries

How the project stays updatable: each replaceable dependency sits behind a seam we own.
See [[ADR-0008 Modular Boundaries For Upstream Sync]]. This note is the working map —
when you add a dependency that someone else ships, add a row here.

## Seams
| Seam | Trait / contract | Lives in | Implementations |
|---|---|---|---|
| Agent runtime | `AgentExecutor` | `crates/core/src/agent.rs` | `MockExecutor`, built-in loop (M1), opencode/Hermes (M2) |
| Tool enforcement | `ToolBroker` | `crates/core/src/runner.rs` | real broker (task 3), scripted brokers in tests |
| Policy | `evaluate(req, mode, rules, ctx) -> Decision` | `crates/permission-engine` | pure function; same inputs → same decision |
| Path sandbox | `PathResolver` | `crates/permission-engine/src/path.rs` | lexical (tests), canonicalizing (runtime) |
| LLM transport | `CompletionClient` | `crates/core/src/llm.rs` | `Gateway` (built-in LLM gateway, `crates/core/src/router/gateway/`), `RouterClient` (one external OpenAI-compatible router), scripted clients in tests |
| Tool execution | `ToolExecutor` | `crates/core/src/broker.rs` | `LocalTools` (`local_tools.rs`), stubs in tests |
| Health probing | `HealthProbe` | `crates/process-manager` | `Gateway` (provider behind the `chat` role), `RouterClient`, scripted probes in tests |
| Git isolation | `WorktreeProvider` | `crates/core` (task 4) | git CLI, no-op for non-git workspaces |
| Task records | `TaskRecorder` | `crates/core` (task 5) | vault Markdown, JSON export |
| Notes graph | `Graph`, `Document`, `Index` | `crates/graph` | Logseq 0.10.9 Markdown file graph; SQLite index cache |
| Wire format | `EventEnvelope` (`v: 1`) | `packages/contracts` | Rust producer, TS consumer, sidecars |
| Sidecars | JSON-RPC envelopes | `services/*` | opencode gateway (M2), Hermes adapter (M2) |

## Rules
- **Depend inward on contracts.** Callers import a trait, not an implementation type.
- **One transport per adapter.** `rusqlite` only in `store.rs` (app state) and `crates/graph/src/index.rs` (the notes index, a separate rebuildable cache); HTTP (`reqwest`, `eventsource-stream`) and the OS keychain (`keyring`) only in `crates/core/src/router/` — the `http_fetch` tool reaches HTTP through the `HttpFetch` seam implemented there — plus the upstream watch's GitHub adapter, `crates/core/src/upstream/github.rs` (public, unauthenticated, read-only; Settings → Upstreams);
  `std::fs` never in `crates/permission-engine`.
- **Conformance first.** Every new implementation of a seam runs the same suite as the
  existing ones before it is wired in.
- **Pin upstream.** `upstream.lock.json` records version + owning adapter + last verified date.
- **Version contracts.** Envelope changes bump `v`; schema changes add a migration.

## Upstream pin file
`upstream.lock.json` at the repo root lists each external component, the revision we build
against, and the adapter responsible. It is the checklist for "what breaks if this updates".
An entry with `watch: {repo, commit}` is also checked once a day by the upstream watch (Settings → Upstreams), which lists the commits and releases published since `commit` and never applies any of them. `make check` keeps `watch.commit` equal to the commit in `pin`.

```json
{
  "9router":      { "pin": "unversioned (user-run)", "adapter": "services/router-adapter", "checked": "2026-11-11" },
  "tauri":        { "pin": "2.x",                   "adapter": "apps/desktop/src-tauri",  "checked": "2026-11-11" },
  "sveltekit":    { "pin": "2.x",                   "adapter": "apps/desktop",            "checked": "2026-11-11" },
  "rusqlite":     { "pin": "0.32 (bundled)",        "adapter": "crates/core/src/store.rs","checked": "2026-11-11" }
}
```

## Review question
For any change: *“if this upstream project released tomorrow, how many files would I touch?”*
If the answer is more than the adapter and its pin, the boundary is wrong.
