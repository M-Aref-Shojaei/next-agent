---
title: Task Tracker
tags: [plan, tracking]
status: live
---

# Task Tracker

All remaining work for the Next Agent project. Each task has a status, the change it makes, and an approval column. UI changes are verified through the Playwright pipeline (`docs/plan/UiTestResults.md`) before merging.

## Remaining M1 tasks

| # | Task | Change | Status | Tests | Approved |
|---|------|--------|--------|-------|----------|
| 1 | Honest structured diff review | Read-only old/new hunk renderer; whole-task Merge/Discard/Keep remain authoritative | approved | `apps/desktop/tests/diff-merge.test.ts`, Playwright smoke suite | ✓ |
| 2 | LLM task summaries | Gateway-backed JSON summarizer with deterministic fallback when router unavailable | approved | Rust compile checks; existing `VaultWriter` fallback tests; adapter tests pending | ✓ |
| 3 | Playwright smoke test | Deterministic browser smoke suite against Vite on port 1420; CI build validation | approved | Playwright: 5 passed | ✓ |
| 4 | Security hardening | Canonical execution paths, worktree identity/stale-review validation, and regression tests from security audit | ready for approval | Workspace Rust tests: 446 core unit tests passed; permission corpus passed | ☐ |
| 5 | Verify authenticated provider chat | Complete the installed app's live 9Router smoke test with an authorized key | awaiting credential setup | Model discovery passed. A test chat returned HTTP 401. A real reply through the installed app is required. | ☐ |
| 6 | CI matrix | macOS/Windows/Linux CI on merged PRs | pending | `.github/workflows/ci.yml` (exists, needs run) | ☐ |
| 9 | Command palette | Cmd/Ctrl+K opens route navigation and Notes retains its search shortcut | shipped in v0.2.0 | Playwright keyboard and focus tests, desktop type checks, and build passed in [PR #3](https://github.com/M-Aref-Shojaei/next-agent/pull/3) | ✓ |
| 10 | Project setup and discovery | Open or create a project and discover its agent instructions, design files, supported vault graph and file tree | shipped in v0.2.0 | Three focused Rust tests and 10 Chromium tests passed. Installed app opened the project and discovered its instructions and design files. | ✓ |
| 11 | Pull request and release | Fix the CI failures and publish verified platform installers from the approved tag | shipped in v0.2.0 | PR and main checks passed. The [release](https://github.com/M-Aref-Shojaei/next-agent/releases/tag/v0.2.0) contains six published installer assets. | ✓ |

## Follow-up work

See the [Product Work Plan](Product%20Work%20Plan.md) for the recommended order and the decisions behind these tasks.

| ID | Task | Depends on | Status | Acceptance evidence | Approved |
|---|---|---|---|---|---|
| REL-01 | Fix the macOS release bundle signature | 11 | planned | A clean download verifies as a signed bundle and installs and launches on an Apple Silicon Mac without local repair. Record signing and notarization requirements for public distribution. | ☐ |
| PROV-01 | Review guided 9Router setup and isolation | — | ready for approval | [PR #10](https://github.com/M-Aref-Shojaei/next-agent/pull/10) passed CI and UI checks. Model import, connection-scoped routing, fallback, and redirect regression tests passed against mock endpoints. | ☐ |
| PS-01 | Load or create project instructions | 10 | planned | Open and New Project read root `AGENTS.md`. New Project creates a starter when absent. Open offers creation if missing. Reopening never overwrites edits, and switching projects reloads instructions from the selected root. | ☐ |
| UX-01 | Audit the desktop UI and UX | CL-01 | planned | Annotated desktop and narrow-window review of project opening, vault and task setup, Design, agent chat, and diff review. Each finding identifies the affected action and a proposed fix. | ☐ |
| UX-02 | Refactor the desktop workspace | UX-01 | planned | Clear primary actions in empty, ready, and error states. Keyboard navigation, project switching, and access to Tasks, Design, review, and Settings pass Playwright for the flows implemented. | ☐ |
| RF-01 | Audit duplicate and redundant code | — | done | [Audit and handoff](Clean%20Code%20Scope%20Audit.md) record desktop and Rust candidates, ownership boundaries, and an independently reviewed first refactor. | ☐ |
| RF-02 | Consolidate verified duplicate code | RF-01 | planned | Focused refactors preserve supported callers, persisted data, and permissions. Focused and full checks pass before redundant paths are removed. | ☐ |
| SEC-01 | Review project isolation and sensitive data in diagnostics | — | planned | Document enforcement and test coverage for project isolation and sensitive data in diagnostics. Register each verified gap as a focused change for independent review. Reuse task 4 for execution paths and worktree review. | ☐ |
| PERF-01 | Measure the main desktop workflows | — | planned | Reproducible baselines identify where native project discovery and vault indexing spend time. Record inputs, commit, hardware, and repeated results. | ☐ |
| PERF-02 | Optimize one measured bottleneck | PERF-01 | planned | A focused change meets the target agreed from the baseline and preserves behavior and resource bounds. Record before-and-after evidence or close with a measured no-op decision. | ☐ |
| PT-01 | Add the vault-backed project Tasks tab | CL-03, UX-01 | planned | Quick-add, acceptance checklists, and status changes update the canonical vault Markdown. The Tasks tab opens from the project sidebar by mouse and keyboard and addresses the Tasks findings from UX-01. Two offline projects reopen with independent task lists. | ☐ |
| PT-02 | Link planned tasks to agent runs and review | PT-01 | planned | An explicit Run with Agent action links one or more runs and the diff to the same task. Retries create no duplicate task, and review approval is separate from run success. | ☐ |
| PT-03 | Link work in Notes and Design | PT-01, DW-02, CL-03 | planned | A note TODO or board item can link to the same task. Switching projects cannot update the other project's task or source artifact. | ☐ |
| PT-04 | Add optional task board and Markdown exchange | PT-01, PT-02 | planned | List and Kanban use the same vault task records and status. A sample Markdown import is previewed, deduplicated, and exported with acceptance criteria intact. | ☐ |

## Design Workspace

See [Design Workspace Plan](Design%20Workspace%20Plan.md) for dependencies and acceptance criteria.

| ID | Task | Depends on | Status | Acceptance evidence | Approved |
|---|---|---|---|---|---|
| CL-01 | Remove misleading duplicate navigation | — | ready for approval | 10 Chromium tests passed, including one current rail destination, narrow-window keyboard access to Settings, and Boards from the palette | ☐ |
| CL-02 | Consolidate project and app navigation | CL-01, DW-02 | planned | Project Design is the canonical workspace and the app component preview has a distinct purpose and label, with old links still reachable | ☐ |
| CL-03 | Discover or attach the vault on Open and New Project | 10 | planned | Existing vault documents, links, and graph load as project context without copying or modifying source files. Explicit selection and fresh project-local setup both work. Existing global-vault artifacts stay readable, and two-project tests prove writes stay scoped. | ☐ |
| CL-04 | Consolidate provider setup | PROV-01 | planned | Saved connections and model roles work through one settings entry, with keychain and fallback regressions covered | ☐ |
| DW-01 | Evaluate OpenPencil in the packaged desktop app | — | planned | Offline edit/save/reopen on supported desktop platforms, a representative .fig compatibility report, and comparable cold-start and bundle measurements | ☐ |
| DW-02 | Add a project-scoped Design Workspace | — | planned | One Design destination links existing boards and prototypes to the active project, with accessible navigation and no migration of existing files | ☐ |
| DW-03 | Edit and preview high-fidelity mockups | DW-01, DW-02 | planned | Open an imported copy or create a mockup, edit reusable components, undo, save and reopen without changing the original | ☐ |
| DW-04 | Author and check clickable flows | DW-02, DW-03 | planned | Board sketch opens an editable mockup, a keyboard-operable flow runner handles expected and failure paths, and invalid links are flagged | ☐ |
| DW-05 | Connect project tokens and AI-assisted design | DW-02, DW-03 | planned | Light/dark token mappings apply to previews, AI changes are reviewable, and existing tokens are updated only through an explicit approved diff | ☐ |
| DW-06 | Implement and verify the pilot workflow | DW-04, DW-05 | planned | Approved prototype becomes a worktree diff, with matching Playwright checks against the running app and owner review evidence | ☐ |

## Planning and maintenance

| ID | Task | Status | Acceptance evidence |
|---|---|---|---|
| PLAN-01 | Assess and decompose the active tracker | Done | 70 work packages cover 25 tracker items with remaining work or evidence. Independently reviewed. Verified parent status preservation, unique IDs, link targets, and an acyclic dependency graph. |
| PLAN-02 | Sequence the four refactors for parallel execution | Done | Four concurrent starting assignments and eight new security and performance cards. Independent review, link and dependency validation, and `pnpm check` passed. |

## Execution table

Use the [Parallel Refactor Plan](Parallel%20Refactor%20Plan.md) to start the UI/UX, security, optimization, and readability lanes together.

Each row is one work package. Open its ID for scope and the specific conditions to mark it **Done**. Update the status, assignee, and evidence in this table. Dependencies include the parent’s gates. See the [Work Package Guide](Work%20Package%20Guide.md) for how to claim, parallelize, review, and close work.

### Release and providers

| ID | Task | Depends on | Status | Assignee | Evidence or blocker |
|---|---|---|---|---|---|
| [2-A](Work%20Packages%20-%20Release%20and%20Providers.md#2-a--verify-valid-structured-summaries-through-the-gateway-adapter) | Verify valid structured summaries through the gateway adapter | — | Planned | — | — |
| [2-B](Work%20Packages%20-%20Release%20and%20Providers.md#2-b--verify-deterministic-summary-fallback) | Verify deterministic summary fallback | 2-A | Planned | — | — |
| [4-A](Work%20Packages%20-%20Release%20and%20Providers.md#4-a--recheck-execution-path-and-permission-regressions) | Recheck execution-path and permission regressions | — | Planned | — | — |
| [4-B](Work%20Packages%20-%20Release%20and%20Providers.md#4-b--recheck-worktree-identity-and-review-outcomes) | Recheck worktree identity and review outcomes | — | Planned | — | — |
| [4-C](Work%20Packages%20-%20Release%20and%20Providers.md#4-c--record-independent-review-and-owner-disposition) | Record independent review and owner disposition | 4-A, 4-B | Planned | — | — |
| [5-A](Work%20Packages%20-%20Release%20and%20Providers.md#5-a--configure-and-test-an-authorized-9router-connection) | Configure and test an authorized 9Router connection | — | Blocked | — | Requires an authorized 9Router credential. |
| [5-B](Work%20Packages%20-%20Release%20and%20Providers.md#5-b--verify-a-reply-in-the-installed-desktop-app) | Verify a reply in the installed desktop app | 5-A | Planned | — | — |
| [6-A](Work%20Packages%20-%20Release%20and%20Providers.md#6-a--define-shared-and-platform-specific-ci-coverage) | Define shared and platform-specific CI coverage | — | Planned | — | — |
| [6-B](Work%20Packages%20-%20Release%20and%20Providers.md#6-b--add-the-missing-platform-checks) | Add the missing platform checks | 6-A | Planned | — | — |
| [6-C](Work%20Packages%20-%20Release%20and%20Providers.md#6-c--record-cross-platform-evidence-on-main) | Record cross-platform evidence on main | 6-B | Planned | — | — |
| [REL-01-A](Work%20Packages%20-%20Release%20and%20Providers.md#rel-01-a--identify-the-bundle-signing-break) | Identify the bundle signing break | 11 | Planned | — | — |
| [REL-01-B](Work%20Packages%20-%20Release%20and%20Providers.md#rel-01-b--correct-packaging-and-document-distribution-requirements) | Correct packaging and document distribution requirements | 11, REL-01-A | Planned | — | — |
| [REL-01-C](Work%20Packages%20-%20Release%20and%20Providers.md#rel-01-c--verify-the-final-installer-on-a-clean-mac) | Verify the final installer on a clean Mac | 11, REL-01-B | Planned | — | — |
| [PROV-01-A](Work%20Packages%20-%20Release%20and%20Providers.md#prov-01-a--review-existing-provider-and-ui-evidence) | Review existing provider and UI evidence | — | Planned | — | — |
| [PROV-01-B](Work%20Packages%20-%20Release%20and%20Providers.md#prov-01-b--record-owner-approval-and-integration-evidence) | Record owner approval and integration evidence | PROV-01-A | Planned | — | — |
| [CL-04-A](Work%20Packages%20-%20Release%20and%20Providers.md#cl-04-a--specify-legacy-connection-compatibility) | Specify legacy connection compatibility | PROV-01 | Planned | — | — |
| [CL-04-B](Work%20Packages%20-%20Release%20and%20Providers.md#cl-04-b--implement-and-verify-the-compatibility-path) | Implement and verify the compatibility path | PROV-01, CL-04-A | Planned | — | — |
| [CL-04-C](Work%20Packages%20-%20Release%20and%20Providers.md#cl-04-c--make-providers-the-single-setup-entry-point) | Make Providers the single setup entry point | PROV-01, CL-04-B | Planned | — | — |

### Project setup and tasks

| ID | Task | Depends on | Status | Assignee | Evidence or blocker |
|---|---|---|---|---|---|
| [PS-01-A](Work%20Packages%20-%20Project%20and%20Tasks.md#ps-01-a--read-and-scaffold-root-instructions) | Read and scaffold root instructions | 10 | Planned | — | — |
| [PS-01-B](Work%20Packages%20-%20Project%20and%20Tasks.md#ps-01-b--load-instructions-for-the-selected-project) | Load instructions for the selected project | 10, PS-01-A | Planned | — | — |
| [PS-01-C](Work%20Packages%20-%20Project%20and%20Tasks.md#ps-01-c--use-project-instructions-during-agent-runs) | Use project instructions during agent runs | 10, PS-01-A | Planned | — | — |
| [CL-03-A](Work%20Packages%20-%20Project%20and%20Tasks.md#cl-03-a--persist-the-project-vault-destination) | Persist the project vault destination | 10 | Planned | — | — |
| [CL-03-B](Work%20Packages%20-%20Project%20and%20Tasks.md#cl-03-b--open-existing-vaults-and-initialize-fresh-ones) | Open existing vaults and initialize fresh ones | 10, CL-03-A | Planned | — | — |
| [CL-03-C](Work%20Packages%20-%20Project%20and%20Tasks.md#cl-03-c--route-project-artifacts-through-the-attachment) | Route project artifacts through the attachment | 10, CL-03-A | Planned | — | — |
| [CL-03-D](Work%20Packages%20-%20Project%20and%20Tasks.md#cl-03-d--offer-vault-setup-from-project-context) | Offer vault setup from project context | 10, CL-03-B, CL-03-C | Planned | — | — |
| [PT-01-A](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-01-a--store-planned-tasks-in-vault-markdown) | Store planned tasks in vault Markdown | CL-03, UX-01 | Planned | — | — |
| [PT-01-B](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-01-b--add-the-project-tasks-route-and-list) | Add the project Tasks route and list | CL-03, UX-01, PT-01-A | Planned | — | — |
| [PT-01-C](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-01-c--verify-offline-persistence-and-project-isolation) | Verify offline persistence and project isolation | CL-03, UX-01, PT-01-B | Planned | — | — |
| [PT-02-A](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-02-a--store-run-links-by-planned-task-id) | Store run links by planned task ID | PT-01 | Planned | — | — |
| [PT-02-B](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-02-b--launch-through-the-existing-agent-flow) | Launch through the existing agent flow | PT-01, PT-02-A | Planned | — | — |
| [PT-02-C](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-02-c--keep-run-success-and-approval-distinct) | Keep run success and approval distinct | PT-01, PT-02-B | Planned | — | — |
| [PT-03-A](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-03-a--define-stable-source-references) | Define stable source references | PT-01, DW-02, CL-03 | Planned | — | — |
| [PT-03-B](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-03-b--link-a-notes-todo-to-a-planned-task) | Link a Notes TODO to a planned task | PT-01, DW-02, CL-03, PT-03-A | Planned | — | — |
| [PT-03-C](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-03-c--link-a-design-board-item-to-a-planned-task) | Link a Design board item to a planned task | PT-01, DW-02, CL-03, PT-03-A | Planned | — | — |
| [PT-04-A](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-04-a--present-status-columns-from-the-same-task-list) | Present status columns from the same task list | PT-01, PT-02 | Planned | — | — |
| [PT-04-B](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-04-b--preview-and-apply-a-markdown-import) | Preview and apply a Markdown import | PT-01, PT-02 | Planned | — | — |
| [PT-04-C](Work%20Packages%20-%20Project%20and%20Tasks.md#pt-04-c--export-and-verify-the-exchange-flow) | Export and verify the exchange flow | PT-01, PT-02, PT-04-B | Planned | — | — |

### UX and refactoring

| ID | Task | Depends on | Status | Assignee | Evidence or blocker |
|---|---|---|---|---|---|
| [CL-01-A](Work%20Packages%20-%20UX%20and%20Refactoring.md#cl-01-a--close-the-duplicate-navigation-review) | Close the duplicate navigation review | — | Planned | — | — |
| [UX-01-A](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-01-a--audit-open-and-new-project) | Audit Open and New Project | CL-01 | Planned | — | — |
| [UX-01-B](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-01-b--audit-navigation-design-chat-and-review) | Audit navigation, Design, chat, and review | CL-01 | Planned | — | — |
| [UX-01-C](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-01-c--turn-findings-into-a-prioritized-change-list) | Turn findings into a prioritized change list | CL-01, UX-01-A, UX-01-B | Planned | — | — |
| [UX-02-A](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-02-a--simplify-project-opening-and-empty-states) | Simplify project opening and empty states | UX-01, PS-01, CL-03 | Planned | — | — |
| [UX-02-B](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-02-b--simplify-chat-and-diff-review) | Simplify chat and diff review | UX-01 | Planned | — | — |
| [UX-02-C](Work%20Packages%20-%20UX%20and%20Refactoring.md#ux-02-c--verify-the-desktop-workflow-at-both-widths) | Verify the desktop workflow at both widths | UX-01, UX-02-A, UX-02-B | Planned | — | — |
| [RF-01-A](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-01-a--inventory-overlapping-desktop-paths) | Inventory overlapping desktop paths | — | Done | Desktop reviewer | [Desktop caller and behavior inventory](Clean%20Code%20Scope%20Audit.md) |
| [RF-01-B](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-01-b--inventory-overlapping-tauri-commands-and-core-paths) | Inventory overlapping Tauri commands and core paths | — | Done | Rust reviewer | [Rust caller and compatibility inventory](Clean%20Code%20Scope%20Audit.md) |
| [RF-01-C](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-01-c--pick-the-first-safe-refactor) | Pick the first safe refactor | RF-01-A, RF-01-B | Done | Coordinator and validation reviewer | [Independent review and first refactor](Clean%20Code%20Scope%20Audit.md) |
| [RF-02-A](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-02-a--lock-down-the-existing-behavior) | Lock down the existing behavior | RF-01 | Planned | — | [Selected scope and checks](Clean%20Code%20Scope%20Audit.md#rf-02-handoff) |
| [RF-02-B](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-02-b--move-callers-to-the-surviving-implementation) | Move callers to the surviving implementation | RF-01, RF-02-A | Planned | — | — |
| [RF-02-C](Work%20Packages%20-%20UX%20and%20Refactoring.md#rf-02-c--remove-the-redundant-implementation-and-recheck) | Remove the redundant implementation and recheck | RF-01, RF-02-B | Planned | — | — |

### Security and optimization

| ID | Task | Depends on | Status | Assignee | Evidence or blocker |
|---|---|---|---|---|---|
| [SEC-01-A](Parallel%20Refactor%20Plan.md#sec-01-a--map-project-and-vault-isolation) | Map project and vault isolation | — | Planned | — | — |
| [SEC-01-B](Parallel%20Refactor%20Plan.md#sec-01-b--review-sensitive-data-in-diagnostics) | Review sensitive data in diagnostics | — | Planned | — | — |
| [SEC-01-C](Parallel%20Refactor%20Plan.md#sec-01-c--schedule-confirmed-security-fixes) | Schedule confirmed security fixes | SEC-01-A, SEC-01-B | Planned | — | — |
| [PERF-01-A](Parallel%20Refactor%20Plan.md#perf-01-a--measure-native-project-discovery) | Measure native project discovery | — | Planned | — | — |
| [PERF-01-B](Parallel%20Refactor%20Plan.md#perf-01-b--measure-vault-indexing-and-refresh) | Measure vault indexing and refresh | — | Planned | — | — |
| [PERF-01-C](Parallel%20Refactor%20Plan.md#perf-01-c--select-one-measured-bottleneck) | Select one measured bottleneck | PERF-01-A, PERF-01-B | Planned | — | — |
| [PERF-02-A](Parallel%20Refactor%20Plan.md#perf-02-a--change-the-selected-hot-path) | Change the selected hot path | PERF-01 | Planned | — | — |
| [PERF-02-B](Parallel%20Refactor%20Plan.md#perf-02-b--compare-and-review-the-candidate) | Compare and review the candidate | PERF-01, PERF-02-A | Planned | — | — |

### Design workspace

| ID | Task | Depends on | Status | Assignee | Evidence or blocker |
|---|---|---|---|---|---|
| [DW-01-A](Work%20Packages%20-%20Design.md#dw-01-a--pin-the-candidate-and-define-repeatable-checks) | Pin the candidate and define repeatable checks | — | Planned | — | — |
| [DW-01-B](Work%20Packages%20-%20Design.md#dw-01-b--package-a-minimal-offline-editor) | Package a minimal offline editor | DW-01-A | Planned | — | — |
| [DW-01-C](Work%20Packages%20-%20Design.md#dw-01-c--measure-fidelity-and-decide-whether-to-integrate) | Measure fidelity and decide whether to integrate | DW-01-B | Planned | — | — |
| [DW-02-A](Work%20Packages%20-%20Design.md#dw-02-a--define-artifact-references-and-project-boundaries) | Define artifact references and project boundaries | — | Planned | — | — |
| [DW-02-B](Work%20Packages%20-%20Design.md#dw-02-b--build-the-project-design-navigation-shell) | Build the project Design navigation shell | DW-02-A | Planned | — | — |
| [DW-02-C](Work%20Packages%20-%20Design.md#dw-02-c--bring-existing-boards-and-directions-into-the-workspace) | Bring existing boards and directions into the workspace | DW-02-B, CL-03 | Planned | — | — |
| [CL-02-A](Work%20Packages%20-%20Design.md#cl-02-a--connect-the-canonical-destinations-and-retain-old-routes) | Connect the canonical destinations and retain old routes | CL-01, DW-02 | Planned | — | — |
| [CL-02-B](Work%20Packages%20-%20Design.md#cl-02-b--verify-desktop-and-narrow-window-navigation) | Verify desktop and narrow-window navigation | CL-01, DW-02, CL-02-A | Planned | — | — |
| [DW-03-A](Work%20Packages%20-%20Design.md#dw-03-a--save-and-reopen-versioned-screens) | Save and reopen versioned screens | DW-01, DW-02 | Planned | — | — |
| [DW-03-B](Work%20Packages%20-%20Design.md#dw-03-b--import-a-bounded-working-copy) | Import a bounded working copy | DW-01, DW-02, DW-03-A | Planned | — | — |
| [DW-03-C](Work%20Packages%20-%20Design.md#dw-03-c--connect-the-editing-controls) | Connect the editing controls | DW-01, DW-02, DW-03-A | Planned | — | — |
| [DW-04-A](Work%20Packages%20-%20Design.md#dw-04-a--validate-flow-data-and-add-a-static-map) | Validate flow data and add a static map | DW-02, DW-03 | Planned | — | — |
| [DW-04-B](Work%20Packages%20-%20Design.md#dw-04-b--link-a-board-sketch-to-saved-screens-and-author-transitions) | Link a board sketch to saved screens and author transitions | DW-02, DW-03, DW-04-A | Planned | — | — |
| [DW-04-C](Work%20Packages%20-%20Design.md#dw-04-c--play-flows-with-keyboard-and-pointer) | Play flows with keyboard and pointer | DW-02, DW-03, DW-04-A | Planned | — | — |
| [DW-05-A](Work%20Packages%20-%20Design.md#dw-05-a--map-existing-tokens-into-screen-previews) | Map existing tokens into screen previews | DW-02, DW-03 | Planned | — | — |
| [DW-05-B](Work%20Packages%20-%20Design.md#dw-05-b--draft-a-screen-or-revision-from-a-board-frame) | Draft a screen or revision from a board frame | DW-02, DW-03 | Planned | — | — |
| [DW-05-C](Work%20Packages%20-%20Design.md#dw-05-c--review-and-approve-token-changes-explicitly) | Review and approve token changes explicitly | DW-02, DW-03, DW-05-A, DW-05-B | Planned | — | — |
| [DW-06-A](Work%20Packages%20-%20Design.md#dw-06-a--record-the-approved-project-opening-prototype) | Record the approved project-opening prototype | DW-04, DW-05 | Planned | — | — |
| [DW-06-B](Work%20Packages%20-%20Design.md#dw-06-b--produce-a-reviewable-implementation-diff) | Produce a reviewable implementation diff | DW-04, DW-05, DW-06-A | Planned | — | — |
| [DW-06-C](Work%20Packages%20-%20Design.md#dw-06-c--compare-the-real-app-with-the-approved-flow) | Compare the real app with the approved flow | DW-04, DW-05, DW-06-B | Planned | — | — |

## Definition of done (M1)

- All steps accepted on macOS, Windows, Linux.
- Security review of permission engine & path checker done.
- Docs in this vault updated; ADRs accepted.
- UI test pipeline green on every PR merge.

## Change log (UI pipeline results)

See [UiTestResults](UiTestResults.md) for the table of UI changes awaiting your approval.
