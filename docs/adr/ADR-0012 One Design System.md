---
title: ADR-0012 One Design System
tags: [adr, ui, design]
status: accepted
---

# ADR-0012 One Design System

- **Status:** accepted (owner rule) · **Date:** 2026-11-15 · **Deciders:** the owner
- Builds on [[ADR-0002 SvelteKit Frontend]]. Affects how [[ADR-0011 Vendor Whole Upstream Codebases on Request]] upstream UIs are shown. Guide: [[Design System]].

## Context
- The desktop app had 12 ad-hoc CSS variables in `app.css` and about 36 Svelte files, each with its own `<style>` block. Raw hex values, font stacks, radii and spacing were repeated by hand, so pages drifted apart.
- Vendored Coucou (ADR-0011) brings a second, separate look: `coucou/src/style.css`, `settings/settings.css`, `hub/hub.css`. Its settings live in their own window.
- The owner's rule (2026-11-15): every screen, ours and vendored upstreams', follows one design system. Upstream settings move into Next Agent's Settings.
- Constraints:
  - The CSP is strict: no inline scripts, no external fonts or CDNs, so system fonts only.
  - We add no new npm dependencies without a strong reason.
  - Light and dark themes follow the OS.

## Decision
1. **Tokens are the only source of raw values.** `apps/desktop/src/lib/ds/tokens.css` defines semantic tokens:
   - colour: surfaces, text, borders, accent, status tones, focus, scrim, brand;
   - type, spacing, radii, shadows, motion, z-index layers and control heights.
   - Light is the default. Dark comes from `prefers-color-scheme`, and `[data-theme]` forces either one. The old variable names stay as aliases until pages move.
2. **Components are ours, in Svelte 5, with no library.** `apps/desktop/src/lib/ds/` holds the building blocks, exported from `index.ts`:
   - controls: Button, TextField, Select, Switch, SegmentedControl, ShortcutField, ColorSwatchGroup and others;
   - layout: SettingsLayout, SettingsSection, SettingsRow;
   - feedback: Banner, Badge, StatusDot, Spinner, ProgressBar.

   Each one meets WAI-ARIA patterns: labels, roles, keyboard support and visible focus. Its pure logic (key capture, roving focus) is unit-tested.
3. **The accent stays blue-indigo. The brand red is the mark only.** Red already means danger: destructive buttons, errors and the Bypass mode. An all-red interactive UI would make every primary action look destructive and blur those warnings. The red gradient orb (the island mascot) stays the brand mark, as `--brand-gradient` and `<BrandMark>`.
4. **A checker enforces the rule.**
   - `scripts/check-design-tokens.mjs` runs in `make check`. It fails on raw colour literals and on `font-family` stacks anywhere under `apps/desktop/src` except `tokens.css`.
   - Today's offenders are listed one per file in `scripts/design-tokens-allowlist.json`, each with a count that may only go down. Vendored Coucou is one pending entry.
   - The list shrinks to zero as pages are restyled.
5. **Upstream UIs move onto it.**
   - Vendored UIs are restyled on these tokens. Their settings become sections of our Settings, built from SettingsLayout, SettingsSection and SettingsRow.
   - Coucou's control types (toggles, segmented pills, sliders, key capture, colour swatches, lists with actions, status rows, about cards) each have a component here for that move.
   - ADR-0011 rule 4 ("keep upstream's form") still applies to their logic. The restyle is a marked `Next Agent:` change.

## Options considered
| | Own tokens + components (chosen) | UI library (e.g. Skeleton, shadcn-svelte, Bits UI) | Tokens only, no components |
|---|---|---|---|
| New dependencies | none | one or more, plus Tailwind for most | none |
| CSP / system fonts | fits | most fit, some need config | fits |
| Fit with Coucou's controls (key capture, swatches) | built for them | partly; the rest are custom anyway | each page re-implements |
| Accessibility | ours to get right (patterns + tests) | mostly given | each page on its own |
| Consistency over time | high: one component per control | high | drifts again |
| Cost | ~30 small components to maintain | upgrades, theming its way | lowest now, highest later |

- **A library** solves accessibility, but it pulls in a styling system the rest of the app doesn't use. It would also be one more upstream to track (ADR-0008), and we would still hand-build key capture, swatches and settings rows.
- **Tokens only** is cheap but repeats today's problem: every page rebuilds its own switch and its own settings row.

## Consequences
- **Easier:**
  - New screens and settings pages are assembled from SettingsRow and friends, so they look alike and need little CSS.
  - Themes change in one file.
  - The style guide (`/settings/design-system`) shows every state in light and dark.
- **Harder:**
  - We own about 30 components, including their accessibility.
  - The dark theme is written twice in `tokens.css`: once for the media query and once for `[data-theme="dark"]`. The checker fails if the two copies differ.
- **Follow-ups:**
  - Restyle each allowlisted page and remove its entry.
  - Restyle Coucou's windows and move its settings into Settings.
  - Remove the legacy variable aliases once no page uses them.

## Action items
1. [x] Tokens, components, style guide page, checker with allowlist, docs.
2. [ ] Restyle our pages on `lib/ds` (shrink the allowlist).
3. [ ] Move Coucou's settings into Next Agent Settings, then restyle its island, pill and hub.
4. [ ] Drop the legacy aliases (`--bg`, `--panel`, `--muted` …).
