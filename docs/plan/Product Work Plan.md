---
title: Product Work Plan
tags: [plan, desktop, design, tasks]
status: proposed
---

# Product Work Plan

Next Agent is one local-first desktop app for opening a project, organizing the work, designing a change, and reviewing the implementation. Common workflows should work in one installation and one project context. AI actions use the configured provider. Manual design and task planning work offline.

## Shipped and in review

| Area | Current state |
|---|---|
| Release and project setup | [PR #3](https://github.com/M-Aref-Shojaei/next-agent/pull/3) shipped in [v0.2.0](https://github.com/M-Aref-Shojaei/next-agent/releases/tag/v0.2.0). Open and New Project, project connections, and keyboard navigation are available. Discovery reports existing instructions, design files, a supported Logseq vault and graph, and the file tree. |
| Local Mac readiness | v0.2.0 installed and the repository opened. The installer checksum matched, but the local bundle needed a signature repair. Keep the public packaging fix tracked as REL-01. |
| Provider readiness | The local 9Router listed models and a default combo was selected. Chat returned HTTP 401 before a successful authenticated reply. Track the live test as task 5. The provider improvements in [PR #10](https://github.com/M-Aref-Shojaei/next-agent/pull/10) remain in review. |
| Navigation cleanup | [PR #11](https://github.com/M-Aref-Shojaei/next-agent/pull/11) removes duplicate rail entries, names Boards correctly in the palette, and carries the [Feature Consolidation Audit](Feature%20Consolidation%20Audit.md). It awaits merge approval. |
| Design authoring tools | Impeccable and Web Design Guidelines were installed for this project's authoring environment. Product Design and Figma were identified as optional authoring plugins. The desktop workflow has no plugin prerequisite. |

## Work in this order

Use the [execution table](Task%20Tracker.md#execution-table) to claim individual work packages. The [Work Package Guide](Work%20Package%20Guide.md) identifies what can start in parallel and where to coordinate shared files.

1. **Make the installed app reliable.** Finish task 5 with an authorized 9Router key and a short live reply. Review PROV-01 and then finish CL-04 so provider setup has one entry point. Track the macOS packaging repair independently as REL-01.
2. **Establish the project workspace and simplify the UI.** Implement PS-01 and CL-03 for instructions and vault setup. Review CL-01 and run UX-01 across project opening, task planning, Design, chat, and review. Implement UX-02 from observed issues. Run RF-01 in parallel and use its evidence to sequence small RF-02 refactors.
3. **Prove the design tools fit inside the app.** Follow [DW-01–DW-06](Design%20Workspace%20Plan.md). Keep one board engine, evaluate the OpenPencil editor locally, and use Next Agent to play linked mockups and test implemented flows. The pilot takes the app's own project-opening flow from sketch to a reviewable Svelte change.
4. **Give each project one backlog.** Once vault attachment is working, add the vault-backed Tasks tab and agent handoff. Link Notes and Design entries to those task records, then consider Kanban and Markdown exchange.

## Opening or creating a project

- Load the selected root's `AGENTS.md` into project context. Reuse existing instructions. Scaffold a starter on New Project where the file is absent. On Open, offer to create a starter for an existing folder with no instructions.
- Discover a supported vault in the project or let the user select an existing vault. Attach it to the project so its Markdown and graph can be read in place. Offer to initialize a project-local vault when a project has none. Show the selected destination before saving project artifacts.
- Preserve documents, boards, and tasks when switching projects. Keep earlier vault files readable during migration and verify that pending writes finish in the project that initiated them.

## Decisions carried forward

- **Provider setup:** Borrow [Hermes's guided endpoint setup](https://hermes-agent.nousresearch.com/docs/user-guide/local-models) and [OpenCode's explicit model selection](https://opencode.ai/docs/providers). Retain the Rust gateway, existing provider settings, configurable roles, and fallback behavior. Keep live authentication verification separate from mock-endpoint coverage.
- **One design workspace:** Keep Excalidraw for boards and evaluate OpenPencil for editable mockups. Reuse the app's Open Design-inspired generation and token workflow. [Penpot](https://github.com/penpot/penpot) remains a reference for component and token handoff, with file exchange as the optional interoperability path.
- **Clutter reduction:** Remove duplicate entry points when a working canonical route exists. Move project files and task state only through an explicit migration with recovery and cross-project tests. Track the remaining candidates in the [Feature Consolidation Audit](Feature%20Consolidation%20Audit.md).

## Built-in project tasks

[Backlog.md](https://github.com/MrLesk/Backlog.md) is the reference for acceptance criteria and reviewable agent handoff. [Vikunja](https://vikunja.io/help/views/) is the reference for quick entry and multiple views of the same records. [Kanboard](https://github.com/kanboard/kanboard) is the reference for a small, readable workflow. Use these patterns in Next Agent's existing desktop stack.

- Make the dedicated Tasks tab the home for planned work, reachable from the project sidebar. Start with quick add, a list, an acceptance checklist, and To Do, In Progress, Review, and Done. Project-vault Markdown owns planned work. SQLite can index those records and retain execution metadata.
- Link planned tasks to the existing execution records. One task can have several runs or retries. The execution timeline owns running, failed, and cancelled statuses. Project work enters Done after the relevant review or explicit completion.
- Keep Notes checkboxes useful on their own. Let the user explicitly link a note TODO or a design board item to a planned task, with one shared task status. Agent work starts only through the existing permission and approval flow.
- Treat exchange with other task formats as explicit import or export. Preview updates before applying them, preserve stable IDs, and keep the existing `docs/plan/Task Tracker.md` readable. Notes, Tasks, and a future Kanban view should refer to the same vault records.

The [Task Tracker](Task%20Tracker.md) contains statuses, dependencies, acceptance evidence, and owner approval. Use the tracker to record changes to this order.
