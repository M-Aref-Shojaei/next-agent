---
title: Feature Consolidation Audit
tags: [plan, design, desktop]
status: proposed
---

# Feature Consolidation Audit

Next Agent should open a project, discover its files and design system, and let a developer move from a sketch to checked code inside a single desktop installation. Optional services belong behind the workflows that use them. The built-in gateway is the provider entry point. A configured model is needed for AI generation.

| Overlap in the current app | Decision | What must survive the change |
|---|---|---|
| **Chat / Code / Source Control.** The activity rail sends Chat and Code to the same route and marks both current. Source Control opens general Settings. The command palette's Code entry opens Boards. | Use Chat for the task workspace and review. Remove the duplicate Code and mislabeled Source Control from the rail. Expose Boards by name in the palette while it has a separate route. | Existing routes, task diffs, Settings, and keyboard navigation. |
| **Rail and sidebar.** The rail and sidebar both advertise Design, Notes, and Usage. Project connections also deep-link to Design and the graph. | Consolidate navigation as the shell is revised. Keep the rail for primary destinations, the sidebar for project and task context, and the palette for keyboard access. Keep contextual deep links where they take the developer directly to a discovered artifact. | Responsive access to destinations, badges, deep links, and one current primary destination per route. |
| **Boards and mockup editing.** Boards already embeds Excalidraw. Adding OpenPencil's board features would offer two canvas editors for the same sketch. | Keep Excalidraw for boards. Evaluate only the OpenPencil editing components needed for screens and .fig exchange. Add the board-to-screen handoff inside Design. | Existing board data, PNG/SVG export, and saved artifact references. Retain /boards as a compatibility entry when navigation is consolidated. |
| **Project design system and app showcase.** Design Studio edits the active project's files. Settings → Design system previews Next Agent's own controls and tokens. | Use Design System for the project and label the Settings surface as a component preview for app contributors. Reuse the existing tokens and preview matrix. | Project token changes remain reviewable. App component previews remain available for regression testing. |
| **Project vault and global vault.** Notes prefers a discovered project vault. Boards currently reads the configured global vault. Both Settings and Notes expose vault setup. | Provide one project storage decision and one setup entry point. Prefer an existing project vault and make the destination explicit before a save. Provide a project-local destination when there is no vault. | Read existing global-vault boards and notes. Keep global-vault access during migration. Verify project switching never redirects pending edits. |
| **Legacy router URL and Providers.** General Settings has a legacy URL form. The built-in provider gateway has its own optional 9Router connection. | Use Settings → Providers for setup. Consolidate the legacy UI once existing settings and keychain entries can be carried forward safely. Review the separate [provider setup PR](https://github.com/M-Aref-Shojaei/next-agent/pull/10) before starting migration work. | Existing credentials, model choices, custom endpoint isolation, and provider fallback. |

The prototype player and real app preview serve separate checks. The first tests what should happen from a mockup and the second tests what actually happens in the code. Keep both reachable from the same flow. Likewise, keep Notes for linked project knowledge and the overlay's meeting capture while comparing their storage and handoff paths. They serve different jobs.

## Removal gates

1. Identify the route, stored data, settings, credentials, and callers behind each candidate before cutting it. Remove stale links and misleading labels first.
2. Establish the canonical workflow and test it on a fresh install and on a project with existing files. Confirm that users can open boards and notes saved by older builds and recover from failed imports or unavailable providers.
3. Keep compatibility redirects and existing data until the replacement is verified. Remove redundant code only after supported callers have moved. Record the migration and rollback behavior in the relevant task.
4. Validate the Design Workspace on a fresh desktop install without adding another account, installing another design app, or starting a background design service. Manual editing and review must work with networking off. Only AI generation uses the configured provider.
