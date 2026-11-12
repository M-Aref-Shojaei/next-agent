---
title: ADR-0007 Minimize Sidecars in M1
tags: [adr]
status: proposed
---

# ADR-0007 Minimize Sidecars in M1

- **Status:** proposed · **Date:** 2026-11-10

## Decision
M1 bundles no Node/Bun/Python runtimes. 9Router is user-run and probed only. `services/*` exist as scaffolds with READMEs and contracts. From M2, sidecars are bundled per-OS via Tauri `externalBin` or required as user installs (decided per service).

## Consequences
Simpler signing/notarization and smaller installers for M1.
