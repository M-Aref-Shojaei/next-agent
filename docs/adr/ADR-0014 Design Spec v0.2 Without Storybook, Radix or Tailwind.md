---
title: ADR-0014 Design Spec v0.2 Without Storybook, Radix or Tailwind
tags: [adr, ui, design]
status: accepted
---

# ADR-0014 Design Spec v0.2 Without Storybook, Radix or Tailwind

- **Status:** accepted · **Date:** 2026-11-18 · **Deciders:** the owner (asked for "the best option"), the coordinating session
- Builds on [[ADR-0002 SvelteKit Frontend]] and [[ADR-0012 One Design System]]. Spec: [[Design Spec v0.2]]. Plan: [[Design Spec v0.2 Plan]].

## Context
- The owner added a design spec (v0.2). It suggests React, Tailwind, Radix and Storybook, but says "do not blindly migrate an existing stack".
- Next Agent is meant to be **downloaded, installed and used** by other people on macOS, Windows and Linux. So what ships must be small, work offline, and pass a strict CSP (`default-src 'self'`). Dev tools don't ship, but each one adds upkeep and supply-chain risk.
- We already have a Svelte 5 design system (`apps/desktop/src/lib/ds/`): about 35 components, tokens in one file, a token checker in `make check`, and a living style guide at Settings → Design system. Coucou is vendored and also uses it.

## Decision
1. **Stay on Svelte 5 and our own `lib/ds` components.** We keep the token names we have and add the spec's missing ones. We do not rename the existing ones, which would mean rewriting every component and the vendored Coucou CSS.
2. **No Radix.** Radix is React-only. Its Svelte port (Bits UI) would bring a second component model into a codebase that already has accessible components. The new hard widgets (command palette, menus and popovers, resizable panels, tree, and the task graph's list view) are written in `lib/ds` following the WAI-ARIA Authoring Practices patterns, and are tested with axe.
3. **No Tailwind.** Semantic CSS variables plus `check-design-tokens.mjs` already give "no hardcoded palette", which is the spec's goal. Tailwind would add a second way to style things and a build step, and Coucou's CSS wouldn't use it anyway.
4. **No Storybook. The in-app style guide is the catalogue.** It ships inside the app, so anyone who installs Next Agent can open it. It grows into the spec's Design mode: a token browser, a component catalogue, a state matrix, light and dark, density, and widths. Storybook would need its own build and add hundreds of dev packages, just to show what that page already shows.
5. **Add tests instead:** Playwright plus `@axe-core/playwright` (dev only) drive the built frontend with the mock API. They take visual snapshots of the style guide and the shell at 800, 1100, 1440 and 1920 px, in light and dark, and run axe on every mode. Component tests stay on `node --test`.
6. **Fonts are bundled, not fetched.** Inter and JetBrains Mono, as variable woff2 files under SIL OFL 1.1, go in `static/fonts/` with their licences in `NOTICE.md`. `default-src 'self'` already allows them, so the CSP is unchanged. The system stack stays as the fallback.
7. **Code editor:** CodeMirror 6 when the Code mode lands. It is framework-free, small, and works with Svelte without wrappers. Monaco is too big for an installed app and needs web workers and extra CSP rules.

## Consequences
- **Good:**
  - The download stays small.
  - There are no new runtime dependencies apart from the fonts (about 0.4 MB) and CodeMirror.
  - Users can see the design system inside the product.
  - The token checker stays the single guard.
- **Cost:**
  - We write and maintain about 5 more complex widgets ourselves.
  - Visual snapshots differ slightly per OS, so they are compared only on macOS CI. Windows and Linux run axe and the smoke tests.
- **Revisit if:** writing our own widgets costs more than a day a month to maintain, or a contributor community wants Storybook-style isolated stories.
