---
name: Next Agent
source: generated from apps/desktop/src/app.css (and component styles) by the Next Agent Design Studio, then edited
tokens: tokens.css
---

# Next Agent design system

This file is the brand contract the Design Studio designs from. Keep it short and specific; edit anything below.

## 1. Visual Theme & Atmosphere

- Restrained, functional interface with light and dark themes that follow the OS.
- Colour is used for meaning (accent, status), not decoration.
- A local-first AI workstation for developers: calm, dense and quiet, so the agent's work (timeline, diffs, approvals) stays the focus.
- Native-feeling: system font stack, 14px base, small radii, 1px borders, almost no shadow.

## 2. Color Palette & Roles

| Token | Light | Dark | Role |
|---|---|---|---|
| `--bg` | `#fafafa` | `#14161a` | Page background |
| `--panel` | `#ffffff` | `#1b1e23` | Raised surface |
| `--sidebar` | `#f1f2f4` | `#181a1f` | Navigation surface |
| `--border` | `#dcdee3` | `#2c3038` | Borders and dividers |
| `--text` | `#1b1d21` | `#e6e8ec` | Body text |
| `--muted` | `#666b75` | `#9097a3` | Secondary text |
| `--accent` | `#3b5bdb` | `#7c93f5` | Accent, primary actions, links |
| `--info` | `#1c7ed6` | `#5aa9f0` | Information |
| `--warn` | `#d9780a` | `#f0a23b` | Warning |
| `--danger` | `#d6293a` | `#ff6b78` | Errors, destructive actions |
| `--ok` | `#2b8a3e` | `#6cc47f` | Success |
| `--focus` | `#3b5bdb` | `#7c93f5` | Focus ring |

## 3. Typography Rules

- Typeface: `--font-sans` = -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
- Typeface: ui-monospace, monospace
- Typeface: ui-monospace, Menlo, monospace
- Sizes in use: `--text-1` (10px), `--text-2` (11px), `--text-3` (12px), `--text-4` (13px), `--text-5` (14px), `--text-6` (16px)
- Headings are tight (`margin: 0`); body text inherits the root size.

## 4. Component Stylings

- Buttons: panel background, 1px border, small radius; the primary button fills with the accent colour.
- Inputs: full width, panel background, same border and radius as buttons.
- Focus: a visible 2px outline in the focus colour on every interactive element.
- _List any other components that have a fixed look._

## 5. Layout Principles

- Spacing steps: `--space-1` (4px), `--space-2` (6px), `--space-3` (8px), `--space-4` (10px), `--space-5` (12px), `--space-6` (14px)
- Prefer grid and flex layouts with `gap` over margins.
- Dense, desktop-first panels; content never touches the window edge.

## 6. Depth & Elevation

- Radii: `--radius-1` (2px), `--radius-2` (4px), `--radius-3` (6px), `--radius-4` (8px), `--radius-5` (10px), `--radius-6` (999px)
- Shadows: `--shadow-1` (0 1px 2px rgb(0 0 0 / 0.06))
- Separate surfaces with borders and background steps before reaching for shadows.

## 7. Do's and Don'ts

- Do use the tokens in `tokens.css` (`var(--name)`) instead of raw values.
- Do keep contrast at WCAG AA or better in both themes.
- Don't introduce new colours, fonts or radii without adding a token.
- Don't load web fonts, images or scripts from the network.

## 8. Responsive Behavior

- Desktop first; layouts must still work down to 375px wide.
- Collapse side panels below about 900px.

## 9. Agent Prompt Guide

When designing for this project: link `tokens.css` or inline its `:root` block, use only its variables for colour, type, spacing and radius, support both colour schemes, and make every control keyboard-reachable with a visible focus ring.

<!-- Read from: app.css, routes/+layout.svelte, routes/+page.svelte, lib/components/ApprovalCard.svelte, lib/components/BoardCanvas.svelte, lib/components/DesignFrame.svelte, lib/components/DiffViewer.svelte, lib/components/IslandMascot.svelte, lib/components/ModeBadge.svelte, lib/components/RightPanel.svelte, lib/components/Sidebar.svelte, lib/components/StatusBar.svelte -->
