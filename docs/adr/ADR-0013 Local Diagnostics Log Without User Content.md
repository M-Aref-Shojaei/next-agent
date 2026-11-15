---
title: ADR-0013 Local Diagnostics Log Without User Content
tags: [adr, logging, privacy]
status: proposed
---

# ADR-0013 Local Diagnostics Log Without User Content

- **Status:** proposed · **Date:** 2026-11-15 · **Deciders:** the owner
- Plan: [[Logging System Plan]]. Related: [[ADR-0004 Permission Engine in Rust Core]] (the log records decisions, not arguments) and [[ADR-0011 Vendor Whole Upstream Codebases on Request]] (Coucou's own log is folded in).

## Context
The owner wants everything that happens in the app logged, so that errors and behaviour can be seen. People's work must never be logged. Today the Rust crates emit `tracing` events that no subscriber receives. Coucou writes its own free-text log, and the UI loses its errors entirely. Free-text logs leak easily: one `%error` that carries a provider's response body can copy a prompt into a file.

## Decision
1. **One local log for the whole app.** It is written as JSON lines under the app data `logs/` folder, rotated daily, and kept for 14 days and at most 100 MB. Only the new `crates/diagnostics` crate writes it. It is never sent anywhere.
2. **Structured events with allowlisted fields.** Each event has a stable name, an area (`target`) and fields from one allowlist. Fields not on the list are dropped, not written. Names such as `prompt`, `content`, `title`, `path`, `token` and `key` are forbidden outright, and `make check` enforces this.
3. **Every string is redacted** (secret shapes, home folder, emails and phone numbers) and cut to 300 characters. Provider and HTTP error bodies, tool arguments and setting values are never logged.
4. **Proof by canary tests.** Sentinel prompts, keys and notes are run through the app's paths, and the tests assert they never appear in the log.
5. **The UI and the Coucou overlay log through the same pipeline.** The UI uses a rate-limited `diag_log` command that applies the same allowlist and redaction on the Rust side. Coucou's `log::line` becomes `tracing` events with target `overlay`.

## Consequences
- **Gain:**
  - errors and behaviour across all areas can be followed;
  - logs can be attached to bug reports without exposing anyone's work;
  - the existing `tracing` calls start working.
- **Cost:**
  - adding a new field means adding it to the allowlist on purpose;
  - some debugging detail, such as a provider's exact error text, is deliberately unavailable;
  - the overlay's free-text messages had to be audited and some rewritten, a recorded deviation from upstream.
- **Rejected:**
  - free-text logging with after-the-fact scrubbing: it fails open;
  - a remote telemetry service: not wanted, and against the local-first principle.
