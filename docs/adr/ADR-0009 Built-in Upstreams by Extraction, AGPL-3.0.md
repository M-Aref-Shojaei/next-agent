---
title: ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0
tags: [adr]
status: proposed
---

# ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0

- **Status:** proposed · **Date:** 2026-11-14
- Supersedes the integration modes in [[Upstream Evaluation]], and the "user-run 9Router" assumption (Q2) in [[ADR-0007 Minimize Sidecars in M1]].
- Amended by [[ADR-0011 Vendor Whole Upstream Codebases on Request]]: an upstream the owner asks to vendor whole (Coucou) is copied, not extracted.
- Resolves Q1 (licence) in [[Risks and Open Questions]].

## Context
The owner wants one app that works on its own. They don't want to start 9Router, Logseq or any other tool separately, or run terminal commands. The upstreams are 9Router (MIT, Node), opencode (TS/Bun), Hermes agent (Python), OpenClaw (TS), Coucou, Logseq (AGPL-3.0, ClojureScript) and Open Design (Apache-2.0, Next.js and Node). Bundling their runtimes would mean shipping Node, Bun and Python and managing several processes, which conflicts with ADR-0007. The owner only wants specific parts of each. For 9Router, those are the "caveman" mode, token reduction, model combos, providers and usage reports. The project will be published free on GitHub.

## Decision
1. **Licence: AGPL-3.0-or-later** for Next Agent. That lets us port code from Logseq (AGPL) and include MIT-licensed code with attribution.
2. **Extract, don't embed.** For each upstream we re-implement or port only the chosen features natively: Rust in `crates/*` for engine logic, Svelte/TS in `apps/desktop` and `packages/*` for UI. We bundle no upstream runtime (Node/Bun/Python) and spawn no upstream process.
3. **Credit every source.** `NOTICE.md` lists each upstream with its licence, copyright holder, repo URL and the commit we studied. Every ported file starts with a header naming the project and file it came from. MIT-licensed code keeps its copyright notice.
4. **Same seams as before.** Extracted features sit behind our existing interfaces: `AgentExecutor`, the tool broker plus permission engine, router client, vault writer, `WorktreeProvider`. A ported agent engine never bypasses the permission engine.
5. **Fixed version per upstream.** `upstream.lock.json` records the upstream commit each port is based on, so later upstream changes can be reviewed and pulled in deliberately.

## Consequences
- The app is self-contained: one installer, no external services except LLM providers and channel APIs.
- More code to own and maintain. Upstream fixes don't arrive automatically; we pull them by hand using `upstream.lock.json`.
- AGPL obligations: anyone who receives the app, or uses a modified version over a network, must be able to get the source. A public GitHub repo satisfies this.
- Some features are hard to port exactly, for example Python-only libraries in Hermes. These are reduced to what our stack supports, and each port's notes record what was left out.

## Alternatives considered
- **Bundle runtimes and run upstreams as managed sidecars.** Rejected: large installers, harder signing, several processes to supervise, and the owner wants only parts of each project.
- **MIT/Apache licence and a clean-room Logseq-like editor.** Rejected: the owner is fine with AGPL, and AGPL allows porting Logseq directly.
