---
title: Design Workspace Plan
tags: [plan, design, desktop]
status: proposed
---

# Design Workspace Plan

## Recommendation

Give developers one project-scoped **Design** destination with **Boards**, **Screens**, **Flows**, and **Design System** views. Keep Excalidraw as the board editor and evaluate OpenPencil as the editable mockup engine. Use the existing Design Studio, model gateway, and worktree review for AI-assisted design and implementation. Keep the navigation and controls consistent with Next Agent's own design system.

| Need | Proposed solution |
|---|---|
| Miro-style brainstorming | Use the existing Excalidraw canvas for notes, frames, connectors, and journey maps. Open a selected board frame as context for a mockup or flow. |
| Figma-style mockups | Create and edit layered screens with text, layout, components, and variables. Import a working copy of an existing .fig file and show unsupported features or visible differences before expecting a round trip. |
| Click through a proposed flow | Link hotspots between saved screen revisions, set a starting screen, and play the flow with keyboard or pointer. Report broken targets and unreachable screens. |
| Check the real project flow | Review the corresponding pages in the running project and run Playwright against the implementation. Compare the same expected outcomes used for the prototype. |
| Claude Design-style assistance | Use the project's design guidance, tokens, and existing components to propose screens, critique them, and suggest targeted revisions through the configured design model. |
| Move from design to code | Send the selected revision, design-system reference, and flow checks to a coding task. Review the resulting worktree diff before merging. |
| Reduce switching between tools | Keep the board reference, editable screen, flow, and implementation task linked to the same project. Preserve working file formats so users can exchange designs with teammates who use external tools. |

OpenPencil documents a headless Vue SDK and a framework-neutral engine. Its [Figma compatibility matrix](https://openpencil.dev/reference/figma-compatibility) lists prototype connections, triggers, and navigation actions as unsupported. Its [file format guidance](https://openpencil.dev/reference/file-format) also calls out round-trip limitations. Next Agent will own the simple clickable flow format and playback. Imported mockups may carry upstream metadata, but the app will only claim playback for links it can validate itself.

## Interaction and data

- On opening a project, Design shows recent artifacts and one primary action suited to the current view. Navigation preserves the selected artifact when switching between board, screen, and flow. Project changes reset the view to that project's saved state.
- Reuse the existing vault-backed boards and design directions. Define a versioned project-relative manifest for artifact references and flow transitions. Use stable artifact IDs and revisions so that renaming or editing a mockup cannot silently change an approved flow. Provide a usable local save location for screens and flows when a project has no vault.
- Keep `DESIGN.md`, `tokens.css`, and the project's existing components authoritative for implementation. Map tokens into the editor and preview with a report of unknown variables and broken aliases. Present imported token changes as a diff for approval.
- Use a bounded hotspot vocabulary for the first release: navigate to a screen, go back, or open and close an overlay. Show which screen is active, offer restart, and provide a keyboard-accessible list of actions alongside the canvas. Keep mockup playback isolated from the development-server preview.
- Treat imported files, references, and generated content as untrusted. Preserve the original file, bound file size and decoding resource use, sanitize renderable content, and deny network access by default in artifact previews. Route code edits and implementation through the existing approval and worktree path.
- The pilot is single-user and local-first. File exchange and reproducible review cover the handoff. Live multiplayer, public sharing, and richer motion can be evaluated after the workflow works end to end.

## Implementation sequence

1. **DW-01 — OpenPencil feasibility.** Pin a published SDK version and verify its artifacts and licenses. Try a lazy-loaded, isolated editor inside the existing Svelte/Tauri shell. Test CanvasKit and fonts offline, input/focus, a representative .fig import and export, save/reopen, and undo. Measure installer size, cold startup, and editor-load time against the current app. If the embed fails the fidelity or platform gates, retain local design exchange and evaluate a supported editor bridge before committing to a replacement.
2. **DW-02 — Workspace navigation.** Add the Design views, shared project context, and links between the existing board and prototype paths using established Svelte components. Verify empty projects, projects without a vault, missing artifacts, and keyboard navigation. This can proceed in parallel with DW-01.
3. **DW-03 — Editable screens.** Add import-as-copy, a starter mockup, canvas editing, and safe save/reopen. Start with frames, text, layout, reused components, and variables. Prove type, layout, and key component fidelity using real fixtures and report unsupported input.
4. **DW-04 — Flow authoring and playback.** Let users connect mockups, preview branches, and attach expected outcomes. Show a static flow map and a runnable prototype. Catch broken links, missing entry points, and unreachable screens before marking a flow ready to review.
5. **DW-05 — Token and AI integration.** Bind current project tokens, generate or revise a screen from a selected board frame, and review editor and token changes before saving. Use the existing design-role gateway. Keep manual editing and preview available when the provider is unavailable. DW-04 and DW-05 can proceed in parallel after DW-03.
6. **DW-06 — Dogfood, then review.** Use Next Agent's own **Open Project → discover connections → start a task** flow. Cover a valid project, an invalid path, an empty project, and an unavailable model. Hand off the approved design to a worktree task, test the real app, and attach Playwright results and screenshots for owner review.

## Verification and approval

- Start with fixture-based tests for import fidelity, save/reopen, token mapping, project isolation, and flow validation. Use a mock design provider in CI and record a separate live-provider check when it is available.
- Run focused Playwright tests first, then `pnpm --filter @next-agent/desktop test:ui`, `pnpm --filter @next-agent/desktop check`, `pnpm --filter @next-agent/desktop build`, `pnpm verify`, and `node scripts/check-design-tokens.mjs`. Run the Rust workspace checks when native integration changes.
- Verify the real packaged editor on macOS, Windows, and Linux. Compare light and dark themes, supported window sizes, keyboard focus, and recovery after failed imports. Keep signed-off evidence in [UiTestResults](UiTestResults.md).
- Review feasibility before adding the editor to the production bundle. Review the pilot UI and worktree handoff before merging implementation.
