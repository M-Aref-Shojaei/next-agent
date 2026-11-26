---
title: Parallel Refactor Plan
tags: [plan, tracking, refactoring]
---

# Parallel Refactor Plan

Four agents can start on one shared baseline. Each assignment produces a bounded review artifact or a small PR. Use the [execution table](Task%20Tracker.md#execution-table) to claim work and record evidence. Choose the commit for the run before measuring or comparing results.

| Scope and suggested model | First assignment | Finish line |
|---|---|---|
| UI/UX · GPT-6.2 Sol, high | CL-01-A reviews the navigation cleanup. Capture a read-only walkthrough of Open and New Project while review is pending. | After CL-01, split UX-01-A/B by workflow. Deliver UX-02-A for project setup and UX-02-B for chat/review separately. UX-02-C records keyboard and narrow-window evidence. Keep Tasks and Design changes with their existing feature owners. |
| Security · GPT-6.2 Astra, high | Run 4-A and 4-B as separate permission and worktree reviews. SEC-01-A/B can review project isolation and diagnostics independently of task 4. | Record existing coverage, fix only a confirmed gap per card, and get independent review. Task 4 keeps its own owner approval step. |
| Optimization · GPT-6.2 Sol, high | Split PERF-01-A/B between native project discovery and vault indexing. Prepare fixtures in parallel and collect timings separately on the same machine. | Select one bottleneck and agree on its target before editing. Demonstrate the gain on the same workload and verify unchanged project behavior, or record why the measurements justify closing the work. |
| Code readability · GPT-6.2 Sol, low | RF-02-A repeats the focused baseline for the selected task-stream helper. | RF-02-B changes one caller. RF-02-C verifies caller behavior and closes the cleanup. Register a separate card for each additional candidate from the [clean-code audit](Clean%20Code%20Scope%20Audit.md). |

## Worktree ownership

- Give each implementation agent a worktree or branch based on the agreed integration commit. Assign one writer per shared file and one integrator for the tracker.
- The readability agent owns the one-expression change in `app.svelte.ts` first. UI and project agents can audit it in parallel. Queue any overlapping write until the small change lands and the next branch rebases.
- Security and performance agents can read project and vault paths concurrently. Coordinate any write to `projects.rs`, `notes.rs`, or shared graph code with PS-01 and CL-03. Review isolation and existing behavior before landing a performance change in that boundary.
- Use the existing UI results page for visible changes. Record baseline and after-change evidence separately. Keep each PR focused so the integration commit can be reverted independently.

## Security follow-up

### SEC-01-A — Map project and vault isolation

- Depends on None.
- Work Trace project identity and read/write destination through `projects.rs`, `notes.rs`, and existing project-owned call sites. Record where boundaries are enforced and which existing synthetic fixtures verify them. Send uncovered planned behavior to PS-01 or CL-03.
- Done when the inventory identifies the normal entry points, guard owners, and current coverage. Record `cargo test --locked -p next-agent-desktop projects::tests` and `cargo test --locked -p next-agent-desktop notes::tests` results or the specific blocker.

### SEC-01-B — Review sensitive data in diagnostics

- Depends on None.
- Work Trace diagnostics from desktop and provider callers into `apps/desktop/src/lib/log.ts` and `crates/diagnostics/src/redact.rs`. Check that redaction and retention coverage includes synthetic credential and user-content markers. Leave provider migration with CL-04.
- Done when the review records the emitters, redaction and retention owners, and covered fields. Record `cargo test --locked -p next-agent-diagnostics` and `node --test apps/desktop/tests/log.test.ts` results or the specific blocker.

### SEC-01-C — Schedule confirmed security fixes

- Depends on SEC-01-A, SEC-01-B.
- Work Independently review the findings. Route covered work to its owner. Give each confirmed gap a separate tracked card with one boundary, a synthetic regression test, a rollback point, and an approval owner.
- Done when each finding has evidence and a disposition. Any new fix stays open under its own ID until focused tests and independent review pass.

## Performance baselines

### PERF-01-A — Measure native project discovery

- Depends on None.
- Work Measure the native `workspace_discover` call on fixed synthetic small and near-limit trees. Record release build, machine, fixture sizes, sample count, median, tail latency, returned entry count, and truncation. Keep the same files and ordering across runs.
- Done when the timing recipe is repeatable and the near-limit and over-limit fixtures check returned counts and the truncation flag. Use `cargo test --locked -p next-agent-desktop projects::tests` for native correctness and Playwright `projects.spec.ts` for browser flow correctness.

### PERF-01-B — Measure vault indexing and refresh

- Depends on None.
- Work Use a generated Markdown vault with known page and link counts. Measure initial indexing, reopen without changes, and a one-page refresh with the same release build. Record wall time and available index timings.
- Done when each workload has repeatable results and the page and link counts match expectations. Run `cargo test --locked -p next-agent-desktop notes::tests` and `cargo test --locked -p next-agent-graph` for correctness.

### PERF-01-C — Select one measured bottleneck

- Depends on PERF-01-A, PERF-01-B.
- Work Compare the baselines and choose one bottleneck with a bounded write scope. Record the measured slow step, file owner, meaningful improvement target, acceptable variance, and required correctness checks.
- Done when a reviewer can repeat the timing and agrees on the target and regression checks, or the recorded measurements support closing the effort without a change.

## First optimization

### PERF-02-A — Change the selected hot path

- Depends on PERF-01.
- Work Implement one change in the selected boundary. Preserve file and graph results, error handling, resource limits, and project isolation. If PERF-01 found no justified change, record that outcome in PERF-02 and mark PERF-02-A/B Deferred with the evidence from PERF-01-C.
- Done when focused regression tests pass and the candidate meets the agreed target on the unchanged fixture.

### PERF-02-B — Compare and review the candidate

- Depends on PERF-02-A.
- Work Rerun the baseline recipe before and after on the same machine. Run the affected Rust or desktop suite and shared checks. Have another reviewer inspect the measurements and fallback behavior.
- Done when recorded timing, correctness, and review evidence support landing the change. Leave the package open or revert the candidate if it misses the target or changes required behavior.
