---
title: Work Package Guide
tags: [plan, tracking]
---

# Work Package Guide

The [Task Tracker](Task%20Tracker.md) is the working queue. Its parent rows define the outcome and owner approval. Its execution table keeps the status of each small work package. Open a package by ID to read its scope, dependencies, and completion checks.

## What the tracker covers

| Assessment | Tracker items |
|---|---|
| Closed feature work | 1, 3, 9, 10, and 11 keep their approved or shipped status. Task 2 remains approved with a separate adapter test follow-up. |
| Review and integration | 4, PROV-01, and CL-01 have existing evidence. Verify it against the relevant commit and record the owner's decision. |
| External inputs | 5 needs an authorized 9Router credential. REL-01 can start with signature diagnosis and needs a clean Apple Silicon test environment for final verification. Public signing and notarization depend on distribution credentials. |
| Implementation and evaluation | All other open tracker rows have packages. Design editor integration and redundant-code removal each start with a decision based on evidence. |

The [Roadmap](Roadmap.md) keeps the later milestone objectives. The execution table breaks down the work already registered in the tracker.

## Claim and close a package

1. Check the parent and card dependencies. A completed parent dependency means its required outcome is available to build on. If it includes integration or owner approval, wait for that gate before landing dependent work. An investigative card can review evidence from an open PR.
2. Put the assignee and `In progress` in the execution row. Give the agent the package ID, scope, allowed files, expected tests, and branch or worktree. A package should produce one reviewable checkpoint. Split new work into linked cards when it grows beyond that.
3. Verify the card's **Done when** conditions. Save the changed commit or document, commands and results, and any required UI or CI evidence in its execution row. Use `In review` while evidence or approval is pending. Use `Blocked` with the exact missing input if progress stops.
4. Mark the package `Done` once its reviewer verifies the evidence and any approval named by the card is recorded. A parent is eligible for closure after all its required packages and the parent acceptance criteria are complete. Preserve the parent approval column and obtain separate authorization before merging or publishing. Optional work can be explicitly deferred with the reason in the tracker.

## Parallel start and handoffs

| Lane | Safe first packages | Handoff and shared files |
|---|---|---|
| Release and provider readiness | 4-A and 4-B review independently. 6-A and REL-01-A can start. PROV-01-A can review the provider PR. Task 5 starts when the credential is available. | Give the shared GitHub workflows one editing owner. Keep provider settings work sequenced after PROV-01. |
| Project setup and tasks | Start PS-01 and CL-03 with separate audits or design notes. | Coordinate one owner for project state and Tauri project commands. Land vault attachment before PT-01. Keep task Markdown records and agent-run metadata separate. |
| UX and redundancy review | Start CL-01-A and RF-01-A/B. Run UX-01 according to the CL-01 handoff. | One editor for the shared navigation shell across UX-02, CL-02, and Tasks or Design route work. RF-02 takes only a candidate that the feature lanes do not already own. |
| Design workspace | DW-01 feasibility and DW-02 planning can start in parallel. | Keep the editor spike isolated from production dependencies and coordinate project artifact storage with CL-03. After editable screens, split flow and token work by files and agree on the saved screen contract before either changes it. |

Use a separate branch or worktree for concurrent changes. Assign one integrator for shared files, tracker status, and PR sequencing. Agents can finish and hand off one package at a time. Read-only audits and test reviews can run concurrently against the same commit.
