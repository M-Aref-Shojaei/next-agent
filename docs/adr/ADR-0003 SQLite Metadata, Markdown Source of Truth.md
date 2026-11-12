---
title: ADR-0003 SQLite Metadata, Markdown Source of Truth
tags: [adr]
status: proposed
---

# ADR-0003 SQLite Metadata, Markdown Source of Truth

- **Status:** proposed · **Date:** 2026-11-10

## Decision
- Knowledge lives in plain Markdown in user-chosen folders; any index is a rebuildable cache.
- App metadata, events, permission rules, and audit trail live in one SQLite DB (`rusqlite`, bundled, WAL) owned by the Rust core.
- The WebView never accesses SQLite directly.

## Consequences
Vaults stay portable to Obsidian/Logseq; DB loss loses history/audit but not knowledge. See [[Data Model]].
