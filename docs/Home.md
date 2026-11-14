---
title: Next Agent — Documentation Vault
tags: [index]
---

# Next Agent

A local-first AI workstation: coding agents working on a local repo, a Markdown knowledge vault, a stateful permission engine, and automatic documentation of completed work. Shell: **Tauri 2** (Rust + SvelteKit).

## Start here
- [[Roadmap]] — milestones M0–M5, sequencing, exit criteria
- [[M1 Vertical Slice]] — step-by-step task breakdown for the first milestone
- [[M1 Progress]] — live task ledger (what is done, tested, and deviated)
- [[Built-in Upstreams Plan]]: waves for building 9Router, opencode, Hermes, OpenClaw, Coucou, Logseq and the Excalidraw design board into the app
- [[Risks and Open Questions]] — things that need a decision or verification
- [[Development Setup]] — toolchain and prerequisites

## Architecture
- [[Architecture Overview]] — components, process model, Input → Process → Output
- [[Project Topology]] — generated Mermaid map of how the crates, packages and services connect
- [[Module Boundaries]] — the seams that keep upstream updates local
- [[Permission Engine]] — modes, sandbox boundaries, command classification, decision order
- [[Worktree Isolation]] — `next-agent/auto-*` worktrees and diff review
- [[Agent Execution]] — `AgentExecutor` trait, tool broker, event streaming
- [[Event Contracts]] — versioned events shared by Rust and TypeScript
- [[Data Model]] — SQLite schema incl. tamper-evident audit log
- [[Router Integration]] — 9Router-compatible endpoint on `:20128`
- [[Vault Documentation Service]] — automatic task records in Markdown
- [[Process Manager]] — sidecar lifecycle and health checks
- [[Desktop UI]] — layout, mode badge, Shift+Tab cycling, diff reviewer

## Decisions (ADRs)
- [[ADR-0000 Template]]
- [[ADR-0001 Tauri 2 as Application Shell]]
- [[ADR-0002 SvelteKit Frontend]]
- [[ADR-0003 SQLite Metadata, Markdown Source of Truth]]
- [[ADR-0004 Permission Engine in Rust Core]]
- [[ADR-0005 Git Worktrees for Auto Mode]]
- [[ADR-0006 Built-in Tool Loop First, Upstream Engines Behind AgentExecutor]]
- [[ADR-0007 Minimize Sidecars in M1]]
- [[ADR-0008 Modular Boundaries For Upstream Sync]]
- [[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]

## Upstream
- [[Upstream Evaluation]] (superseded) · [[9Router]] · [[OpenRouter]] · [[OmniRoute]] · [[opencode]] · [[Hermes Agent]] · [[OpenClaw]] · [[Coucou]] · [[Logseq]] · [[Open Design]] · [[Design Studio Sources]] — opencode, 9router, openclaw, hermes-agent, coucou, Logseq
- [[Diagramming and Topology]] — tools for box diagrams and dependency maps of this project

## Guides & reference
- [[Conventions]]
- [[Glossary]]
- [[Task Record Template]] — format the app writes into user vaults

## Vault conventions
- Every note has YAML frontmatter (`title`, `tags`, optional `status`).
- Links use `[[Note Title]]`; file names equal note titles.
- ADRs are immutable once `accepted`; supersede them with a new ADR.
