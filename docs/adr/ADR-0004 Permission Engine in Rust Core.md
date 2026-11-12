---
title: ADR-0004 Permission Engine in Rust Core
tags: [adr, security]
status: proposed
---

# ADR-0004 Permission Engine in Rust Core

- **Status:** proposed · **Date:** 2026-11-10

## Context
Agents (built-in or upstream) and LLM output are untrusted. Sandbox guarantees must not depend on each upstream's own permission logic.

## Decision
- All side-effecting tools are executed by a Rust tool broker after evaluation by `crates/permission-engine` (pure, table-tested).
- Upstream engines are configured to delegate tool execution to the broker.
- M1 enforcement is policy-level; M2 adds OS-level process sandboxing (Seatbelt / Landlock+bubblewrap / Windows restricted tokens) for spawned commands.

## Consequences
- (+) One auditable choke point; same rules for every executor and channel.
- (−) Upstream engines that can't delegate tools need patches/forks.
- (−) Until M2, an allowed command can itself escape the workspace — documented in UI and [[Risks and Open Questions]] (R1).
