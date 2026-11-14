---
title: Built-in Upstreams Plan
tags: [plan]
status: proposed
---

# Built-in Upstreams Plan

Goal: one self-contained app. No separate programs, no terminal setup. We extract the parts of each upstream we want, rebuild them inside Next Agent and credit the source ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]], `NOTICE.md`). The research behind each part is in [[9Router]], [[opencode]], [[Hermes Agent]], [[OpenClaw]], [[Coucou]] and [[Logseq]].

## How the work runs
- Each **session** is a separate Claude Code session with its own git branch and worktree. Sessions in the same wave work in parallel on separate areas.
- The coordinating session (this one) merges each wave and fixes integration issues. It runs the full checks (`cargo test`, clippy, `make check`, node tests, svelte-check), updates [[M1 Progress]] and asks the owner before merging anything.
- Model per session: **Opus** for security-critical work or work spanning several modules, **Sonnet** for well-specified UI and adapters.
- Time estimates are agent working time per session. The owner's time is reviewing at each wave gate: about 15 minutes per wave.

## Shared rules for every session
- Port, don't bundle. Bundle no Node, Bun or Python runtime, and spawn no upstream program.
- Every ported file starts with: `// Ported from <project> (<licence>, © <holder>): <upstream path> @ <commit>`.
- Every tool call goes through the broker and the permission engine. Nothing bypasses them.
- New SQLite tables are added as migrations in `crates/core/src/store.rs`, the only place `rusqlite` is allowed. Each session claims its migration number in its brief to avoid clashes.
- Secrets go in the OS keychain (`KeyringSecret`), never in SQLite or files.
- Don't edit `docs/plan/M1 Progress.md`. Report progress and deviations in the final message.

## Wave 1: foundations (5 sessions in parallel)

| # | Session | Source | Model | Est. | Done when |
|---|---|---|---|---|---|
| 1 | **LLM gateway**: providers with keys in the keychain; OpenAI ↔ Anthropic ↔ Gemini translation; combos with fallback and cooldown; Caveman and Ponytail prompts; RTK tool-output compression; Anthropic `cache_control`; usage and cost recording. Settings UI for providers, keys, default model, combos and token savers, plus a Usage page. | 9Router | Opus | 4–6 h | Real model chat works from the app with no external router. The Settings page has model and key fields. Usage page shows tokens and cost. |
| 2 | **Agent engine**: context compaction; tiered edit matching; shadow-git per-step undo; Markdown agent profiles (`build`, `plan`, `general`, `explore`); one subagent tool (merges opencode's and Hermes' designs); prompt-cache rules. | opencode, Hermes | Opus | 4–6 h | Long tasks compact instead of failing. Edits tolerate whitespace and quote differences. Per-step undo works. Subagents run under the permission engine. |
| 3 | **Notes engine**: Rust parser and writer for Logseq Markdown with a round-trip test corpus; SQLite index (pages, blocks, refs, properties, FTS5); file watcher; task records and journals moved to the Logseq layout (`journals/`, `pages/`). | Logseq | Opus | 4–5 h | A real Logseq graph opens and saves without changes. Search and backlinks queries work. |
| 4 | **Overlay**: second always-on-top window showing the active task, mode and pending approvals, with approve and deny; notch on macOS, corner widget on Windows and Linux; click-through. | Coucou (code ideas only, no assets) | Sonnet | 2–3 h | Approving from the overlay unblocks a running task. |
| 5 | **Design board**: Miro-style canvas using Excalidraw (MIT), embedded in a Svelte page; boards saved in the vault as `boards/*.excalidraw`; sticky notes, frames, shape libraries for wireframes; import Mermaid diagrams onto a board. | Excalidraw | Sonnet | 2–3 h | Create, edit and save a board from inside the app. Boards appear in the vault. Mermaid import works. |

**Gate 1 (owner):** try real chat, the overlay and the design board. Approve the merge.

## Wave 2: features on top (3 sessions)

| # | Session | Source | Model | Est. | Done when |
|---|---|---|---|---|---|
| 6 | **Notes UI**: outliner editor (indent, outdent, move, collapse, undo); backlinks panel; journals feed; simple queries compiled to SQL; graph view; Mermaid rendering in notes. | Logseq | Sonnet | 4–6 h | Daily notes work in the app the way they do in Logseq. |
| 7 | **Memory and skills**: SKILL.md skills (list, view, manage, curator); curated memory (`MEMORY.md`, `USER.md`); session store with FTS search tool; background review after each task; cron jobs. | Hermes | Opus | 4–6 h | The agent saves and reuses skills and memories across sessions. Scheduled jobs run. |
| 8 | **Channels core and Telegram**: channel trait and supervisor; DM pairing and allow-lists; session routing; group gating; outbound chunking; approvals on desktop or overlay only; channel tasks never in bypass mode. | OpenClaw | Opus | 4–6 h | Message the agent on Telegram after pairing. Approvals appear on the desktop. |

**Gate 2 (owner):** try notes, skills and memory, and Telegram. Approve the merge.

## Wave 3: breadth and hardening

| # | Session | Model | Est. |
|---|---|---|---|
| 9 | Discord, Slack and Gmail channels | Sonnet | 4–6 h |
| 10 | Security review: permission engine, broker, channels, keychain use, ported parsers | Opus | 2–3 h |
| 11 | CI on macOS, Windows and Linux; signed builds; a "first run" setup screen with no terminal | Sonnet | 2–3 h |

## Not planned (recorded so nothing is forgotten)
- Penpot (MPL-2.0): a full Figma-style design tool, but it needs its own server (Postgres, Redis), so it can't be built in. The Excalidraw board covers wireframes and flows.
- tldraw: its licence requires a watermark and a paid production key, so we don't use it.
- WhatsApp's unofficial protocol: risk of account bans. Use the official Cloud API later if needed.
- Logseq whiteboards, PDF annotation, Datalog queries, plugin API.
- LSP and MCP: MCP may come later via the `rmcp` crate.
