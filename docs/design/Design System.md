---
title: Design System
tags: [design, ui, guide]
status: accepted
---

# Design System

The one design system for every Next Agent screen. Decision and trade-offs: [[ADR-0012 One Design System]].

- **Code:** `apps/desktop/src/lib/ds/`: `tokens.css` holds the tokens; the components are exported from `index.ts`.
- **Living style guide:** Settings → Design system (`/settings/design-system`). It shows every token and every component state, with a Light/Dark preview switch.
- **Guard:** `scripts/check-design-tokens.mjs` runs in `make check`.

## The rule

> **All UI uses these tokens and components: ours and every vendored upstream's (Coucou today).**
> No raw colours, no font stacks, no one-off controls. Upstream settings move into Next Agent's Settings, built from `SettingsLayout`, `SettingsSection` and `SettingsRow`, and their own settings windows go away.

- Raw values live only in `lib/ds/tokens.css`.
- A page that needs a new value adds a **semantic** token there, not a literal in its own `<style>`.
- A page that needs a new control adds a component here, with its states shown on the style-guide page.
- Exceptions are temporary. They are listed in `scripts/design-tokens-allowlist.json`, one entry per file, and the list only shrinks.

## Quick start

```svelte
<script lang="ts">
  import { SettingsLayout, SettingsSection, SettingsRow, Switch, Select } from "$lib/ds/index.ts";
  let island = $state(true);
  let model = $state("smart");
</script>

<SettingsLayout title="Overlay" nav={[{ id: "general", label: "General", href: "#general" }]} current="general">
  <SettingsSection id="general" title="General">
    <SettingsRow label="Show the island" description="The pill around the notch.">
      {#snippet children(ctl)}<Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={island} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Default model">
      {#snippet children(ctl)}
        <Select id={ctl.id} bind:value={model} options={[{ value: "fast", label: "Fast" }, { value: "smart", label: "Smart" }]} />
      {/snippet}
    </SettingsRow>
  </SettingsSection>
</SettingsLayout>
```

In CSS, use tokens only:

```css
.panel { background: var(--color-surface); border: var(--border-width) solid var(--color-border);
         border-radius: var(--radius-lg); padding: var(--space-4); box-shadow: var(--shadow-sm); }
```

## Principles

1. **Calm and dense.** This is a desktop tool used all day: quiet surfaces, one accent, information first.
2. **Semantic, not literal.** Use `--color-danger`, not "red". Tokens say what a value is for, so themes change in one place.
3. **Status by colour plus text or icon.** Never colour alone, for colour-blind users and screen readers.
4. **Accessible by default.** Every control has a name, a role, keyboard support and a visible focus ring (`--focus-ring`). Motion respects `prefers-reduced-motion`.
5. **Native first.** System fonts (the CSP forbids web fonts), native `<dialog>`, `<select>`, radio and checkbox inputs where they behave well.
6. **Accent is blue; brand is red.** The interactive accent is blue-indigo. Red means danger, so the brand red (the island orb) appears only as the brand mark: `--brand-gradient` and `<BrandMark>`.

## Themes

- Light is the default; dark follows `prefers-color-scheme`.
- `<html data-theme="light">` or `data-theme="dark"` forces a theme.
- `tokens.css` writes the dark values twice: once inside the media query and once for `[data-theme="dark"]`. The checker fails if the two blocks differ.

## Tokens

### Colour (`--color-*`)
| Group | Tokens | Use |
|---|---|---|
| Surfaces | `bg`, `surface`, `surface-sunken`, `surface-raised`, `surface-overlay`, `surface-hover`, `surface-active`, `surface-inverse` | `bg`: the app background. `surface`: cards and panels. `surface-sunken`: sidebars, wells, segmented tracks. `surface-raised`: the selected segment, popovers. `surface-overlay`: dialogs. `surface-hover` / `surface-active`: interaction. `surface-inverse`: tooltips. |
| Text | `text`, `text-secondary`, `text-tertiary`, `text-disabled`, `text-inverse` | `text-secondary`: descriptions and hints. `text-tertiary`: placeholders and meta. |
| Borders | `border-subtle`, `border`, `border-strong` | `border-subtle`: row dividers. `border-strong`: hover and unchecked controls. |
| Accent | `accent`, `accent-hover`, `accent-active`, `accent-subtle`, `accent-text`, `on-accent` | Primary actions, selection, links (`accent-text`). `on-accent`: text on a solid accent. |
| Status | `{info,ok,warn,danger}`, `-subtle`, `-border`, `-text`, `on-{tone}` | Solid colour for dots and bars; `-subtle` + `-border` + `-text` for banners and badges (AA contrast). |
| Neutral | `neutral`, `neutral-subtle`, `neutral-border`, `neutral-text` | Grey badges and dots. |
| Misc | `focus`, `scrim`, `selection`, `control-thumb` | Focus ring, dialog backdrop, text selection, switch and slider thumbs. |
| Brand | `brand`, `brand-2`, plus `--brand-gradient` | The red orb only. |

Derived: `--focus-ring` (a two-layer box-shadow) and `--focus-ring-inset`.

Legacy aliases (do not use in new code): `--bg --panel --sidebar --border --text --muted --accent --info --warn --danger --ok --focus`.

### Type
| Token | Value |
|---|---|
| `--font-sans` | system UI stack (`-apple-system`, `Segoe UI`, `system-ui` …) |
| `--font-mono` | `ui-monospace`, `SF Mono`, `Menlo`, `Consolas` … |
| `--font-size-xs / sm / md / lg / xl / 2xl` | 11 / 12 / **14** (base) / 16 / 20 / 26 px |
| `--font-weight-regular / medium / semibold / bold` | 400 / 500 / 600 / 700 |
| `--line-height-tight / normal / relaxed` | 1.25 / 1.45 / 1.6 |
| `--letter-spacing-caps` | 0.06em, for small uppercase labels |

### Space, shape, depth
| Token | Values |
|---|---|
| `--space-N` | N × 4px: `0`, `0-5` (2), `1` (4), `1-5` (6), `2` (8), `3` (12), `4` (16), `5` (20), `6` (24), `8` (32), `10` (40), `12` (48) |
| `--radius-xs / sm / md / lg / xl / full` | 3 / 4 / 6 / 10 / 14 / 999 px |
| `--border-width`, `--border-width-strong` | 1px, 2px |
| `--shadow-sm / md / lg / xl` | resting card / raised control / popover / dialog |
| `--control-height-sm / md / lg` | 24 / 30 / 36 px, for buttons, fields and segmented controls |
| `--content-max-width`, `--settings-nav-width` | 880px, 200px |

### Motion and layers
| Token | Values |
|---|---|
| `--duration-instant / fast / normal / slow` | 60 / 120 / 200 / 320 ms; about 0 under reduced motion |
| `--ease-standard / out / in` | movement / entering / leaving |
| `--z-base … --z-tooltip` | base 0, raised 1, sticky 10, dropdown 100, overlay 200, modal 300, popover 400, toast 500, tooltip 600 |

## Components

All components are in `$lib/ds/index.ts`, written in Svelte 5 runes with TypeScript. "Bindable" means you can use `bind:`. Most components also pass through native attributes (`...rest`).

### Actions
| Component | Props | Use when |
|---|---|---|
| **Button** | `variant` primary\|secondary\|ghost\|danger, `size` sm\|md\|lg, `loading`, `block`, `disabled`, snippets `icon`, `iconEnd` | Any text action. Use one primary per view. Danger is only for destructive actions. Ghost is for toolbars and low emphasis. `loading` keeps the width, sets `aria-busy` and blocks clicks. |
| **IconButton** | `label` (required), `variant` ghost\|secondary\|danger, `size`, `pressed` | Icon-only actions. `label` is the accessible name and tooltip. `pressed` makes it a toggle. |
| **Icon** | `name` (see `ICON_NAMES`), `size`, `label` | The built-in stroke icons. They are decorative unless `label` is set. |

### Inputs
| Component | Props | Use when |
|---|---|---|
| **Field** | `label`, `hint`, `error`, `required`, `hideLabel`, `id`, snippet `children({id, describedby, invalid, labelId})` | Labelling any custom control. It wires the label and `aria-describedby` for you. |
| **TextField** | bindable `value`, `label`, `hint`, `error`, `hideLabel`, `size`, `mono`, snippets `prefix` and `suffix` | Single-line text. With `label` it wraps itself in a Field; without one, pass `aria-label`. |
| **TextArea** | bindable `value`, `label`, `hint`, `error`, `rows`, `mono`, `resize` | Multi-line text. |
| **Select** | bindable `value`, `options: {value,label,disabled?}[]`, `label`, `placeholder`, `size`, `error` | Choosing one of more than about 5 options. Use a native select. |
| **ShortcutField** | bindable `value` ("Ctrl+Shift+K"), `label` or `id`, `requireModifier`, `clearable`, `platform`, `record`, `onrecording` | Recording a global shortcut. Click, then press keys. Esc cancels; Backspace clears. Pass Coucou's recorder as `record` to keep its AltGr rules. `onrecording` pauses global shortcuts while recording. |
| **Kbd** | `keys`, `platform`, `size` | Showing a shortcut as keycaps (⌘⇧K on a Mac, Ctrl+Shift+K elsewhere). |

### Choices
| Component | Props | Use when |
|---|---|---|
| **Switch** | bindable `checked`, `label`, `size` sm\|md, `onchange` | An on/off setting that applies at once. In a SettingsRow, leave out `label` and pass `id`. |
| **Checkbox** | bindable `checked` and `indeterminate`, `label`, `description` | Choices confirmed later (a form), multi-select, select-all. |
| **RadioGroup** | bindable `value`, `options` (with `description`), `label` (legend), `orientation`, `hint` | Choosing one of 2–5 options that need explanation. |
| **SegmentedControl** | bindable `value`, `options`, `label` or `labelledby`, `size`, `block` | Choosing one of 2–4 short options that switch a mode or view (Coucou's "pills"). Arrow keys move the choice. |
| **Slider** | bindable `value`, `min`, `max`, `step`, `showValue`, `format` | Choosing from a continuous range (volume, opacity). `format` also sets `aria-valuetext`. |
| **ColorSwatchGroup** | bindable `value`, `swatches: {value,label,color}[]`, `label` or `labelledby` | Picking a colour from a palette. The colours are the caller's data; the control itself uses tokens. |

### Settings
| Component | Props | Use when |
|---|---|---|
| **SettingsLayout** | `title`, `description`, `nav: {id,label,href,badge?}[]`, `current`, snippets `top`, `actions`, `children` | Every settings page: section nav on the left and content on the right. Below a 640px container width, the nav moves to a strip on top. |
| **SettingsSection** | `title`, `description`, `id`, snippets `actions`, `footer`, `children` | A group of rows in one bordered box, with a heading and optional header actions. |
| **SettingsRow** | `label`, `description`, `error`, `stacked`, `disabled`, `id`, snippet `children({id, describedby, invalid, labelId})` | **The main building block.** Label and description on the left, the control on the right. Put `id` on single controls; put `labelledby={labelId}` on groups. Use `stacked` for wide controls. It stacks by itself when narrow. |

### Feedback
| Component | Props | Use when |
|---|---|---|
| **Banner** | `tone` info\|ok\|warn\|danger, `title`, `ondismiss`, snippets `actions`, `children` | A page-level message. Danger is `role="alert"`; the other tones are `role="status"`. |
| **Badge** | `tone` neutral\|accent\|info\|ok\|warn\|danger, `variant` subtle\|solid\|outline, `size` | A short label or count. Not clickable. |
| **StatusDot** | `tone`, `text` or `label`, `pulse` | Connection or run state in rows ("Installed", "Online"). Always give text or a label. |
| **Spinner** | `size`, `label` | Short, unknown waits. Use `label=""` when a parent already announces the wait. |
| **ProgressBar** | `value` (omit for indeterminate), `max`, `label`, `hideLabel`, `showValue`, `tone`, `format` | Measurable progress: indexing, downloads, budgets. |
| **EmptyState** | `title`, `description`, `compact`, snippets `icon`, `actions` | An empty list or page. Say what will appear there and offer the one next step. |

### Content and overlays
| Component | Props | Use when |
|---|---|---|
| **Card** | `title`, `description`, `variant` default\|hero\|subtle, `padding`, `level`, snippets `media`, `actions`, `children` | Grouping content. `hero` is for about and intro cards, with `<BrandMark>` as `media`. |
| **List / ListRow** | List: `label`, `bordered`. ListRow: `title`, `description`, `meta`, `selected`, `disabled`, `onclick` or `href`, snippets `leading`, `actions`, `children` | Lists of things with per-item actions (bots, rules, providers). The row body can be one button or link; actions stay separate buttons. |
| **Tabs** | `tabs: {id,label,disabled?,badge?}[]`, bindable `value`, `label`, snippet `children(activeId)` | Switching views of the same object. Arrow keys, Home and End move between tabs. Use SettingsLayout's nav for page-level navigation instead. |
| **Dialog** | bindable `open`, `title`, `description`, `confirmLabel`, `cancelLabel`, `tone` default\|danger, `busy`, `hideConfirm`, `onconfirm` (may be async), `oncancel`, snippet `children` | Confirming a consequential action. It uses native `<dialog>`, which traps focus and closes on Esc. `danger` focuses Cancel first. |
| **Tooltip** | `text`, `placement` top\|bottom, snippet `children({describedby})` | Explaining an icon. Never hide required information in a tooltip. |
| **BrandMark** | `size`, `label` | The red orb as a static brand mark. |

Pure helpers (tested in `apps/desktop/tests/ds.test.ts`):

- `recordPress`, `pressedKey`, `formatKeys`, `shortcutParts` and `shortcutLabel` (`shortcut.ts`);
- `rovingIndex` (`keyboard.ts`).

## Do and don't

| Do | Don't |
|---|---|
| `color: var(--color-text-secondary)` | `color: #666` or `color: gray` |
| `font-family: var(--font-mono)` | `font-family: Menlo, monospace` |
| Build settings pages from `SettingsRow` | Hand-roll a `.row { display: grid }` per page |
| `<Button variant="danger">` for Delete | A red primary button for a normal save |
| Pair status colour with text: `<StatusDot tone="ok" text="Online" />` | A bare green dot |
| `<IconButton label="Remove bot">` | An icon button without a name |
| Add a semantic token when you need a new value | Add a one-off `--my-page-blue` in a page |
| Show a new component's states on `/settings/design-system` | Ship a component no one can see in light and dark |

## Enforcement: `scripts/check-design-tokens.mjs`

- **Scope:** `.svelte`, `.css` and `.ts` files under `apps/desktop/src`, except `lib/ds/tokens.css`.
- **What fails:**
  - Colour literals in a CSS value: `#hex`, `rgb()`, `hsl()`, `oklch()` and the other colour functions.
  - Named colours in colour properties.
  - `font-family` with anything other than `var(--font-…)` or a CSS-wide keyword.
  - In Svelte files, the checker reads `<style>` blocks, `style="…"`, `style:prop="…"`, and colour attributes (`fill`, `stroke`, `stop-color` …). Scripts are scanned as TS.
  - In TS files, CSS-like declarations (`backgroundColor: "#fff"`, `"color: red"`).
- **Allowlist:** `scripts/design-tokens-allowlist.json` has one entry per offending file, with `max`, the count today.
  - The check fails when a file goes over its `max`, when an allowlisted file is clean (remove its entry), or when an allowlisted file no longer exists.
  - It prints a note when a count drops, so you can lower `max`.
  - `apps/desktop/coucou/**` is one pending entry: the vendored Coucou UI, which gets restyled and has its settings moved.
- **Dark-theme parity:** the two dark blocks in `tokens.css` must match.
- **Run it:** `node scripts/check-design-tokens.mjs --list` prints every finding. Tests: `scripts/tests/design-tokens.test.ts`.

## Moving a page or an upstream onto the system

1. Replace its controls with `lib/ds` components, and settings forms with SettingsLayout, SettingsSection and SettingsRow.
2. Replace raw values with tokens. If none fits, add a semantic token to `tokens.css` (light and both dark blocks) and show it on the style-guide page.
3. Run `node scripts/check-design-tokens.mjs`. Remove the file's allowlist entry when it is clean.
4. For vendored upstream code, mark each change `Next Agent:` (ADR-0011), so a re-sync stays a plain diff.
