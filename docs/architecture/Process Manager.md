---
title: Process Manager
tags: [architecture]
status: draft
---

# Process Manager

Crate: `crates/process-manager`. Supervises long-running sidecars and external endpoints.

## Responsibilities
- Spec per service: command, args, env, cwd, runtime (`node`, `bun`, `uv run`, binary), health probe (HTTP URL or JSON-RPC ping), restart policy (backoff, max restarts).
- States: `stopped → starting → healthy ⇄ degraded → crashed`; events emitted to UI.
- Logs captured to rotating files under `<app_data>/logs/<service>.log`.
- Graceful shutdown on app exit (SIGTERM → timeout → kill; Windows job objects so children die with the app).
- External (unmanaged) services like user-run 9Router: probe-only.

## M1 usage
Only the router health probe. Sidecars arrive in M2 ([[ADR-0007 Minimize Sidecars in M1]]).
