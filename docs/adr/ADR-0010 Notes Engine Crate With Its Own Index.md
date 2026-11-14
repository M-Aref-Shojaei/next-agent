---
title: ADR-0010 Notes Engine Crate With Its Own Index
tags: [adr, vault, logseq]
status: proposed
---

# ADR-0010 Notes Engine Crate With Its Own Index

- **Status:** proposed · **Date:** 2026-11-14
- Builds on [[ADR-0003 SQLite Metadata, Markdown Source of Truth]], [[ADR-0008 Modular Boundaries For Upstream Sync]] and [[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]. Design: [[Notes Engine]].

## Context
- We are building Logseq's Markdown graph format into the app. That means a parser and writer, a folder layout, an index with FTS5 search, and a file watcher.
- The index is a **cache of the user's Markdown**. It can be rebuilt from the files at any time. A user may open several graphs, and each one needs its own index.
- `crates/core` is the app core (store, agent, router). The core's vault writer needs the Logseq file-name and journal rules. The desktop app needs everything.
- Until now, `rusqlite` was allowed only in `crates/core/src/store.rs`. That file holds durable app state: tasks, the audit chain and settings, with versioned migrations.

## Decision
1. The notes engine is its own crate, `crates/graph` (`next-agent-graph`). It does not depend on `crates/core`. `crates/core` depends on it, as does the desktop app.
2. The engine keeps its index in **its own SQLite file**, one per graph, in the app data folder (`graph-index/graph-<hash>.sqlite`). It never writes it into the vault or into the core store.
3. `crates/graph/src/index.rs` is the second allowed owner of `rusqlite`. Each crate still has exactly one file that talks to SQLite, and `scripts/check-boundaries.mjs` enforces this.
4. The index schema has its own version. On a mismatch, the cache is dropped and rebuilt instead of migrated.

## Options considered

| | A: own crate + own index (chosen) | B: tables in core `store.rs` | C: engine inside `crates/core` |
|---|---|---|---|
| Complexity | Low: self-contained crate | Medium: core migrations, one shared DB | Low at first, grows |
| Rebuild / drop cache | Delete one file | Must not touch app state in the same DB | As for B |
| Several graphs | One file each | Needs a graph id on every row | As for B |
| Coupling | Core depends on graph, not the reverse | Graph would depend on core's `Store` | Core grows a large ported upstream |
| Upstream re-sync (ADR-0008) | Diff one crate | Spread over core files | Spread over core files |
| Rule change | Second `rusqlite` owner | None | None |

- **B** keeps the single-owner rule. But it mixes a disposable cache with the audited, migrated app state. It forces a graph id onto every row, and it makes the notes engine depend on core.
- **C** puts a large port of AGPL code into the core, where other sessions work in parallel, and blurs the sync boundary for one upstream.

## Consequences
- Deleting `graph-index/` is always safe. The next `graph_open` rebuilds the index.
- The engine can be tested and reused on its own: corpus, property tests and watcher tests run without the core store.
- Two crates link `rusqlite` 0.32 (bundled). They must stay on the same version so there is a single `libsqlite3`.
- Index writes are not in the core audit chain. User edits saved through `graph_save_page` are plain file writes. Agent-made vault writes still go through `VaultWriter` and are audited.
- Revisit if the index ever has to hold data that cannot be rebuilt (for example, block history). That data would belong in the core store.
