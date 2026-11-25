---
title: Work Packages - Release and Providers
tags: [plan, release, providers, tasks]
status: proposed
---

# Work Packages - Release and Providers

The [Task Tracker](Task%20Tracker.md) owns parent status and approval. Task 2 remains approved with adapter evidence pending. Tasks 4 and PROV-01 are ready for approval, task 5 awaits an authorized credential, task 6 is pending, and REL-01 and CL-04 are planned. Each package is small (up to half a day) unless marked medium (up to one day). The named role leads the work and records evidence. The repository owner approves review and release decisions. Hosted job queues, credential setup, and owner review are outside the estimates.

## 2 — LLM task summaries

### 2-A — Verify valid structured summaries through the gateway adapter

- Depends on None.
- Work as a Rust test engineer. Start with the existing `task_summarizer_tests` in `apps/desktop/src-tauri/src/tasks.rs`. Record their evidence. Add a focused adapter-to-`VaultWriter` fixture, or link an existing integration test that verifies the same handoff.
- Done when the focused desktop test passes under `cargo test -p next-agent-desktop task_summarizer_tests` and shows the resulting task document uses the provider summary.

### 2-B — Verify deterministic summary fallback

- Depends on 2-A.
- Work as a Rust test engineer. Use the existing malformed-response and provider-error adapter tests and the core writer fixture. Extend the fixture if the adapter-to-writer fallback remains unverified. Verify that task documentation still completes through the existing deterministic `VaultWriter` path in `crates/core/src/vault.rs`.
- Done when the desktop adapter tests and `cargo test -p next-agent-core vault::tests` pass, and the tracker links the adapter evidence without reopening the approved feature.

## 4 — Security hardening

### 4-A — Recheck execution-path and permission regressions

- Depends on None.
- Work as a Rust security reviewer. Recheck the existing execution-path boundary tests in core and policy tests in the permission engine. Match the tests to expected canonical-path and permission decisions and record any remaining coverage gaps.
- Done when `cargo test -p next-agent-core --lib` and `cargo test -p next-agent-permission-engine` pass and the evidence identifies the tests covering the audited boundaries.

### 4-B — Recheck worktree identity and review outcomes

- Depends on None.
- Work as a Rust security reviewer. Trace the existing review guard from `apps/desktop/src-tauri/src/tasks.rs` into `crates/core/src/worktree.rs`. Verify existing tests cover correct worktree identity and stale review rejection alongside valid review actions.
- Done when `cargo test -p next-agent-core worktree` and the applicable desktop review tests pass, with the expected outcomes recorded.

### 4-C — Record independent review and owner disposition

- Depends on 4-A, 4-B.
- Work as an independent reviewer. Review the boundary evidence and identify actionable gaps or remaining risks. Submit the tracker evidence for the owner’s approval.
- Done when the review names the tested commit and commands, records the results and any open findings, and records the owner’s decision separately.

## 5 — Verify authenticated provider chat

### 5-A — Configure and test an authorized 9Router connection

- Depends on None.
- Work with the environment owner. In the installed desktop app, check that the selected provider URL and model are the intended ones and enter an authorized credential through Settings → Providers.
- Done when the connection test lists the intended model through the configured endpoint. Record the app version, selected connection, and test outcome without recording the credential.

### 5-B — Verify a reply in the installed desktop app

- Depends on 5-A.
- Work as a desktop QA reviewer. Select the verified model or combo as the chat target, submit a short prompt in an approved local test project, and check the provider response.
- Done when a real authenticated reply appears in the app and the tracker records the version, endpoint alias, selected model and result. Keep the task blocked if authentication still returns HTTP 401.

## 6 — CI matrix

### 6-A — Define shared and platform-specific CI coverage

- Depends on None.
- Work as a CI maintainer. Compare `.github/workflows/ci.yml`, `ui.yml`, and the release workflows with [Branch and PR Rules](Branch%20and%20PR%20Rules.md). Distinguish the existing Linux shared runs from the installer matrix on release tags. Identify the missing cross-platform evidence for a merged commit.
- Done when a coverage table names the trigger, command, runner, and required status for each check, with repository rules and release-only publishing accounted for.

### 6-B — Add the missing platform checks

- Depends on 6-A.
- Work as a CI maintainer. Medium. Extend the shared validation path to produce the agreed macOS, Windows, and Linux results. Reuse existing install and check commands and keep version-tag publication in `release.yml`.
- Done when a candidate run produces explicit results on all three platforms and the logs establish that the required steps ran on each runner. Record formatter, lint, test and build commands as applicable to the agreed coverage.

### 6-C — Record cross-platform evidence on main

- Depends on 6-B.
- Work as a release QA reviewer. Validate all required workflow results for the same merged commit after the normal approval process.
- Done when the tracker links the main commit and its green macOS, Windows, and Linux checks. Keep the task pending while any promised platform remains unverified.

## REL-01 — Fix the macOS release bundle signature

### REL-01-A — Identify the bundle signing break

- Depends on 11.
- Work as a macOS release engineer. Inspect the published v0.2.0 Apple Silicon installer and the macOS build path in `.github/workflows/release.yml`. Verify its checksum and inspect the mounted app and its nested components.
- Done when the checklist records the affected bundle component, the packaging stage that changed it, and the output of `codesign --verify --deep --strict` and `spctl --assess --type execute` against the downloaded app.

### REL-01-B — Correct packaging and document distribution requirements

- Depends on 11, REL-01-A.
- Work as a macOS release engineer. Medium. Correct signing order and make sure final packaging preserves signatures. Record the approved signing and notarization setup and credential ownership for public distribution. Preserve the existing version-tag publication gate.
- Done when a nonpublishing candidate macOS build passes strict signature verification and the release notes distinguish the verified signing setup and required public-distribution credentials.

### REL-01-C — Verify the final installer on a clean Mac

- Depends on 11, REL-01-B.
- Work as a macOS QA reviewer. Download the approved release candidate on Apple Silicon, verify its checksum and signature, and test install and first launch without changing the bundle or its security attributes.
- Done when the release record links the tested build and checksum, records signature and Gatekeeper outcomes and, where applicable, stapling verification, and captures a successful clean launch.

## PROV-01 — Review guided 9Router setup and isolation

### PROV-01-A — Review existing provider and UI evidence

- Depends on None.
- Work as a provider reviewer. Read [PR #10](https://github.com/M-Aref-Shojaei/next-agent/pull/10), its hosted checks, the provider Playwright spec, and the gateway tests. Check the coverage for model import, endpoint isolation, fallback, and redirect handling.
- Done when an independent review links the tested SHA and green checks, names the relevant tests, records any unresolved findings, and distinguishes mock endpoint results from the live test in task 5.

### PROV-01-B — Record owner approval and integration evidence

- Depends on PROV-01-A.
- Work as a maintainer. Present the reviewed PR and the current checks to the owner. Record the approval decision and verify integration before handing the result to CL-04.
- Done when the tracker links the owner-approved merge and its commit evidence. While approval is pending, keep the package In review and record the outstanding decision.

## CL-04 — Consolidate provider setup

### CL-04-A — Specify legacy connection compatibility

- Depends on PROV-01.
- Work as a provider engineer. Inventory `router.url`, `router.model`, the implicit legacy account, saved keys, combos, and role selection in `apps/desktop/src-tauri/src/gateway.rs` and `crates/core/src/router/gateway/mod.rs`. Plan how legacy and explicit connections are resolved during transition.
- Done when an upgrade fixture matrix covers legacy-only, explicit-only, and mixed setups and records expected model selection, fallback, keychain behavior, retry safety, and recovery.

### CL-04-B — Implement and verify the compatibility path

- Depends on PROV-01, CL-04-A.
- Work as a Rust engineer. Medium. Implement the agreed compatibility path in the provider configuration and keychain integration. Keep existing model and role settings readable and preserve explicit endpoint isolation.
- Done when `cargo test -p next-agent-core router::gateway` and `cargo test -p next-agent-desktop gateway` pass with old-store fixtures and cover keychain failure, repeat loading or migration, and provider fallback.

### CL-04-C — Make Providers the single setup entry point

- Depends on PROV-01, CL-04-B.
- Work as a desktop UI engineer. Consolidate the legacy URL form into the Providers workflow and retain a working route to the canonical settings screen. Preserve discoverability of saved endpoints and model roles.
- Done when focused provider Playwright tests and `pnpm --filter @next-agent/desktop check` pass for new and upgraded configurations. Record fresh and upgraded setup evidence in [UI Test Results](UiTestResults.md).
