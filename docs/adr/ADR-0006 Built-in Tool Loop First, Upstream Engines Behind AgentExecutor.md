---
title: ADR-0006 Built-in Tool Loop First, Upstream Engines Behind AgentExecutor
tags: [adr, agents]
status: proposed
---

# ADR-0006 Built-in Tool Loop First, Upstream Engines Behind AgentExecutor

- **Status:** proposed · **Date:** 2026-11-10

## Context
The vertical slice must prove permissions, worktrees, and documentation end-to-end. Integrating opencode/Hermes first would put upstream integration risk on the critical path.

## Decision
M1 ships `MockExecutor` + a minimal Rust `BuiltinExecutor` (OpenAI-compatible tool calling via the router). opencode and Hermes integrations come in M2 as additional `AgentExecutor` implementations.

## Consequences
(+) Faster slice, no sidecar in M1. (−) Built-in agent is less capable; acceptable for the slice.
