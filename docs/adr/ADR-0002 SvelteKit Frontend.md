---
title: ADR-0002 SvelteKit Frontend
tags: [adr]
status: proposed
---

# ADR-0002 SvelteKit Frontend

- **Status:** proposed · **Date:** 2026-11-10

## Decision
SvelteKit (Svelte 5) + Vite + TypeScript, `adapter-static` with SSR disabled (SPA in Tauri). CodeMirror 6 for editors and diffs.

## Consequences
- (+) Small bundle, fast rendering of long timelines.
- (−) Fewer ready-made components than React; some upstream UI (e.g. opencode web parts) can't be reused directly.
