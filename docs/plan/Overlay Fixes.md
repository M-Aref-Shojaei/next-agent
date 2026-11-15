---
title: Overlay Fixes
tags: [plan, overlay, coucou]
status: proposed
---

# Overlay Fixes

Issues the owner reported in the island and pill overlay (Coucou, vendored in `apps/desktop/coucou/`, [[ADR-0011 Vendor Whole Upstream Codebases on Request]]). Each task is written so an agent can be dispatched for it alone. The visual layer follows [[ADR-0012 One Design System]]. Coucou's behaviour stays as upstream, apart from the deviations the owner asked for, which are recorded in `docs/upstream/Coucou.md`.

## How the work runs
- **One agent per task**, each in its own worktree branch. The coordinating session merges, runs all checks (`cargo test --workspace`, `make lint`, `make check`, `node scripts/test.mjs`, svelte-check and the build), then deletes the finished agent and its worktree.
- **Edits inside vendored Coucou code** are marked `// Next Agent:` with the reason.
- **Skills:** `engineering:debug` for task 1; `simplify` at the end of each task; `run` or the real app for visual checks on a MacBook with a notch.

## Task 1: right-side pills slide under the notch on hover
- **Reported (2026-11-15):** "when I hover over it, the side pills, which are on the right, go behind the notch."
- **Model:** Sonnet. **Est.:** 1–2 h.
- **Lead to check first:**
  - `apps/desktop/coucou/src/core/layout.ts` sizes the compact island from fixed PC constants: `NOTCH_W = 184`, `COMPACT_W = 288` ("NOTCH_W + 104"), `EXPANDED_W = 640`.
  - On a MacBook the real notch is wider and depends on the model, so content placed beside an assumed 184 px notch lands under the camera housing.
  - The real notch geometry is known on the Rust side: `crates/macos-panel/src/geometry.rs` (`NO_NOTCH_WIDTH`/`NO_NOTCH_HEIGHT`) and the safe-area insets of the screen with the notch (`auxiliaryTopLeftArea`/`auxiliaryTopRightArea`).
  - Check how the hover (peek) and compact layouts get their width, and whether the measured notch width reaches the frontend.
- **Reference:** upstream's Swift layout at the pinned commit `0d7fe58` (`Louis-CFM/coucou`, `NotchBuddy/` sources). Upstream sizes the bar from the measured notch on a Mac. Match that, so this is a port fix, not a redesign.
- **Done when:**
  - On the owner's MacBook, the hidden, compact (hover) and expanded states never draw pills, the dot or text inside the notch rectangle.
  - Screens without a notch and external monitors keep today's sizes.
  - There is a unit test for the size function, covering a notch wider than 184 px, no notch, and an external display.
  - Before and after screenshots of hover are attached to the report.
- **Owns:**
  - `apps/desktop/coucou/src/core/layout.ts` and the view or CSS files that place the right card (`views/views.ts` `.right`/`.pills`, `style.css`);
  - the bridge that passes the notch size from Rust, if one is missing (`coucou/src-tauri/src/platform/macos.rs`, `crates/macos-panel`);
  - but not the `unsafe` blocks in `crates/macos-panel` without coordinator review.

## Task 2: the logo mark on the pills
- **Requested (2026-11-15):** use the Next Agent logo, or a form of it, for the pills.
- **Model:** Sonnet. **Est.:** 1–1.5 h.
- **Assets** are already made from the owner's logo, in `apps/desktop/src-tauri/icons/source/`:
  - `mark.png` (256×256): the logo's centre hexagon with the brain, in full colour.
  - `mark-mono.png`: the hexagon as one colour, with the brain lines cut out, for tinting.
  - `mark-lines.png`: the brain lines alone, in white.
  - `logo.png`: the full logo.
- **What the pills draw today:**
  - Each pill on the island's sides is a "mini bot" (`coucou/src/mochi/minibots.ts`, `createMiniBot(task, 24)` in `views/views.ts` `buildPill`).
  - Next Agent replaced Coucou's character with a coloured orb (`coucou/src/mochi/orb.ts`), tinted with the agent's identity colour (`core/pills.ts`).
- **Change:**
  - Draw the mark instead of the orb in the pills: `mark-mono` tinted with the pill's identity colour, with `mark-lines` in white on top. This keeps the per-agent colours, such as Claude Code's orange, while every pill wears the Next Agent shape. Preview: hexagons in orange, green and indigo with white brain lines.
  - Keep the mini bot's motion and state cues: the bob, working, approval and finished. Apply them to the mark, with the same timings.
  - Copy the three PNGs into the overlay's assets, e.g. `apps/desktop/coucou/src/assets/brand/`, and load them once.
  - Also use the mark wherever else the orb stands in for Coucou's character at small sizes (greeting, hub sidebar), if the owner agrees in review. Ask in the report; don't change those unasked.
- **Done when:**
  - The pills show the tinted mark at 24 px in light and dark and on Retina, and their animations and badges are unchanged.
  - The design-token check passes. The identity colours stay as data, as today.
  - `docs/upstream/Coucou art to replace.md` records which art spots the mark now fills.
- **Owns:** `coucou/src/mochi/minibots.ts`, `coucou/src/mochi/orb.ts` (only if shared drawing moves), `coucou/src/assets/brand/**` and `docs/upstream/Coucou art to replace.md`.
- **If the owner prefers bespoke art:** generate a pill icon with this prompt, then save it as `apps/desktop/src-tauri/icons/source/pill.png`. The task is the same with that file.

  > A minimal app icon glyph for a small status pill, 512×512, transparent background. A rounded hexagon, pointy top, holding a simple stylised brain whose centre line branches into three circuit nodes, drawn as clean white strokes of even weight. The hexagon is a flat fill with a soft diagonal gradient from royal blue (#136AEF) to cyan (#0B9FE1) to teal (#11CAB2), with a hint of violet (#5548EB) at the bottom. No text, no shadow, no outer glow, no background, crisp vector edges, readable at 24 pixels. Matches a logo of a hexagonal network hub with circular nodes.

## Order
Tasks 1 and 2 touch different files, apart from possibly `views/views.ts`, so they can run in parallel. If both need `views.ts`, run task 1 first.
