---
title: ADR-0001 Tauri 2 as Application Shell
tags: [adr]
status: accepted
---

# ADR-0001 Tauri 2 as Application Shell

- **Status:** accepted (project requirement) · **Date:** 2026-11-10

## Context
Need a cross-platform (macOS/Windows/Linux) desktop shell with privileged local orchestration, small footprint, and strong control of what the UI can do.

## Decision
Tauri 2 with a Rust core. React Native is excluded. Electron is excluded unless an upstream dependency cannot work without it — any such exception requires its own ADR.

## Consequences
- (+) Small binaries, Rust safety for the permission/sandbox core, capability-based IPC.
- (+) Multi-window support for the M3 overlay.
- (−) System WebViews differ (WKWebView, WebView2, WebKitGTK) → test UI on all three; avoid bleeding-edge CSS/JS.
- (−) Node/Python upstreams must run as sidecars.

## Alternatives
Electron (heavier; rejected by requirement), React Native desktop (rejected by requirement), native per-OS (cost).
