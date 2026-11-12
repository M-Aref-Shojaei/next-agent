---
title: Conventions
tags: [guide]
---

# Conventions

- **Commits:** Conventional Commits (`feat(permission-engine): …`).
- **Rust:** `cargo fmt`, `clippy -D warnings`, `thiserror` for library errors, `anyhow` only in app binary, `tracing` for logs.
- **TS:** strict mode, ESLint + Prettier, no `any` in `packages/contracts`.
- **Contracts:** change Rust types → regenerate `packages/contracts`; breaking changes bump envelope `v`.
- **Security-relevant code** (`permission-engine`, path checks, broker) needs tests for every new rule and a reviewer.
- **Docs:** new architectural decision → new ADR from [[ADR-0000 Template]]; update relevant architecture note in the same PR.

## Modularity (always on)
- Every external dependency sits behind a seam we own — see [[Module Boundaries]] and [[ADR-0008 Modular Boundaries For Upstream Sync]].
- Callers import a trait or contract, never another module's internals; a crate's public surface is its `lib.rs`.
- One transport per adapter: `rusqlite` only in `store.rs`, HTTP only in the router client, no `std::fs` in `crates/permission-engine`.
- New implementation of a seam ⇒ it must pass the same conformance suite as the existing ones.
- Pin upstream versions in `upstream.lock.json` with the owning adapter; a bump touches only that adapter.
- Contract changes bump a version (`EventEnvelope.v`, `MIGRATION_Vn`) and ship a migration or shim.
