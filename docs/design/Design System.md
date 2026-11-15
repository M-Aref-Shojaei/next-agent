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
| Fill | `fill-1` … `fill-5` | Translucent washes over any surface: hover, tracks, hairlines, scrollbar thumbs. About 4 / 7 / 10 / 16 / 30 % of the text colour (dark ink in light, white in dark). |
| Island | `island` | The fixed black of Coucou's island: it joins the hardware notch, so it is the same in every theme. Also the ink of the island's shadows and masks. |
| Brand | `brand`, `brand-2`, plus `--brand-sheen` and `--brand-gradient` | The red orb only. `--brand-sheen` is the white highlight over the conic gradient; `IslandMascot` reuses it for its other tones. |

Derived: `--focus-ring` (a two-layer box-shadow) and `--focus-ring-inset`.

Legacy aliases (do not use in new code): `--bg --panel --sidebar --border --text --muted --accent --info --warn --danger --ok --focus`.

### Type
| Token | Value |
|---|---|
| `--font-sans` | system UI stack (`-apple-system`, `Segoe UI`, `system-ui` …), then the fallbacks for Devanagari, Bengali, Arabic and CJK (`Nirmala UI`, `Microsoft YaHei`, the Noto families) |
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
| `--control-height-xs / sm / md / lg` | 20 / 24 / 30 / 36 px; xs is for dense in-row controls (an outline twisty), the rest for buttons, fields and segmented controls |
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
| **IconButton** | `label` (required), `variant` ghost\|secondary\|danger, `size` xs (20px)\|sm\|md\|lg, `pressed` | Icon-only actions. `size="xs"` is for dense rows. `label` is the accessible name and tooltip. `pressed` makes it a toggle. |
| **Icon** | `name` (see `ICON_NAMES`), `size`, `label` | The built-in stroke icons. They are decorative unless `label` is set. |

### Inputs
| Component | Props | Use when |
|---|---|---|
| **Field** | `label`, `hint`, `error`, `required`, `hideLabel`, `id`, snippet `children({id, describedby, invalid, labelId})` | Labelling any custom control. It wires the label and `aria-describedby` for you. |
| **TextField** | bindable `value` (`string`, `number`, `null` or `undefined`), `label`, `hint`, `error`, `hideLabel`, `size`, `mono`, bindable `ref` (the `<input>`), snippets `prefix` and `suffix` | Single-line text. With `label` it wraps itself in a Field; without one, pass `aria-label`. |
| **TextArea** | bindable `value` (also `null` or `undefined`), `label`, `hint`, `error`, `rows`, `mono`, `resize`, bindable `ref` (the `<textarea>`) | Multi-line text. |
| **Select** | bindable `value` (also `null` or `undefined`: nothing chosen, the placeholder shows), `options: {value,label,disabled?}[]`, `label`, `placeholder`, `size`, `error` | Choosing one of more than about 5 options. Use a native select. |
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
| **SettingsLayout** | `title`, `description`, `nav: {id,label,href,badge?}[]`, `current`, snippets `top`, `actions`, `children` | Every settings page: section nav on the left and content on the right. Below a 640px container width, the nav moves to a strip on top. The header's `actions` wrap under the title when there is no room, so a control never overlaps the description. |
| **SettingsSection** | `title`, `description`, `id`, snippets `actions`, `footer`, `children` | A group of rows in one bordered box, with a heading and optional header actions. |
| **SettingsRow** | `label`, `description`, `error`, `stacked`, `disabled`, `id`, snippet `children({id, describedby, invalid, labelId})` | **The main building block.** Label and description on the left, the control on the right. Put `id` on single controls; put `labelledby={labelId}` on groups. Use `stacked` for wide controls. It stacks by itself when narrow. |

### Feedback
| Component | Props | Use when |
|---|---|---|
| **Banner** | `tone` info\|ok\|warn\|danger, `title`, `ondismiss`, `compact`, `role`, snippets `actions`, `children` | A page-level message. Danger is `role="alert"`; the other tones are `role="status"`. `compact` is one small line with less padding, for notices inside a log or timeline. Inside a region that already announces (`role="log"`), pass `role="status"` so an error is not announced twice. |
| **StatTile** | `label`, `value`, `delta`, `hint`, `tone` neutral\|accent\|info\|ok\|warn\|danger, snippet `children` | One headline number with its label (usage totals). `delta` ("+8%", "3 failed") takes the tone colour and always says it in words. Lay tiles out in a CSS grid: `repeat(auto-fit, minmax(150px, 1fr))`. |
| **Badge** | `tone` neutral\|accent\|info\|ok\|warn\|danger, `variant` subtle\|solid\|outline, `size` | A short label or count. Not clickable. |
| **StatusDot** | `tone`, `text` or `label`, `pulse` | Connection or run state in rows ("Installed", "Online"). Always give text or a label. |
| **Spinner** | `size`, `label` | Short, unknown waits. Use `label=""` when a parent already announces the wait. |
| **ProgressBar** | `value` (omit for indeterminate), `max`, `label`, `hideLabel`, `showValue`, `tone`, `format` | Measurable progress: indexing, downloads, budgets. |
| **EmptyState** | `title`, `description`, `compact`, snippets `icon`, `actions` | An empty list or page. Say what will appear there and offer the one next step. |

### Content and overlays
| Component | Props | Use when |
|---|---|---|
| **Card** | `title`, `description`, `variant` default\|hero\|subtle, `padding`, `level`, snippets `media`, `actions`, `children` | Grouping content. `hero` is for about and intro cards, with `<BrandMark>` as `media`. |
| **List / ListRow** | List: `label`, `bordered`. ListRow: `title`, `description`, `meta`, `tooltip`, `selected`, `selectedAs` pressed\|current, `disabled`, `onclick` or `href`, snippets `leading`, `actions`, `children` | Lists of things with per-item actions (bots, rules, providers). The row body can be one button or link; actions stay separate buttons. `tooltip` sets the native title (the full path behind a short name). A selected button row is `aria-pressed` (a toggle); use `selectedAs="current"` in navigation lists for `aria-current`. |
| **Table** | `columns: {key,label,align?,sortable?,value?,sortValue?,nowrap?}[]`, `rows`, `rowKey`, `label` (required), bindable `sortKey` and `sortDir`, `empty`, `emptyDescription`, `onsort`, snippet `cell(row, column)` | Data that is clearly tabular (usage by model, commits). Sortable columns get a header button and `aria-sort`; `align="end"` right-aligns numbers in tabular figures; `sortValue` sorts by a raw value behind a formatted one; empty values sort last. With no rows it shows an `EmptyState`. At narrow widths it scrolls sideways inside a focusable region. For lists of things with actions, use ListRow. |
| **Tabs** | `tabs: {id,label,disabled?,badge?}[]`, bindable `value`, `label`, snippet `children(activeId)` | Switching views of the same object. Arrow keys, Home and End move between tabs. Use SettingsLayout's nav for page-level navigation instead. |
| **Dialog** | bindable `open`, `title`, `description`, `confirmLabel`, `cancelLabel`, `tone` default\|danger, `size` md\|lg (lg: a diff or a table), `busy`, `hideConfirm`, `confirmDisabled`, `onconfirm` (may be async), `oncancel`, snippet `children` | Confirming a consequential action. It uses native `<dialog>`, which traps focus and closes on Esc. `danger` focuses Cancel first and sets `role="alertdialog"`. `confirmDisabled` keeps Confirm off until the dialog's own condition holds (a typed name matches). |
| **Tooltip** | `text`, `placement` top\|bottom, snippet `children({describedby})` | Explaining an icon. Never hide required information in a tooltip. |
| **BrandMark** | `size`, `label` | The red orb as a static brand mark. |

Pure helpers (tested in `apps/desktop/tests/ds.test.ts`):

- `recordPress`, `pressedKey`, `formatKeys`, `shortcutParts` and `shortcutLabel` (`shortcut.ts`);
- `rovingIndex` (`keyboard.ts`);
- `sortRows`, `compareValues` and `nextSort` (`table.ts`).

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
  - In TS files, CSS-like declarations (`backgroundColor: "#fff"`, `"color: red"`), also inside style strings (`style: "color:#fff;width:1px"`). `${expr}` in a template and an unquoted `CONSTANT` are code, not colours.
- **Allowlist:** `scripts/design-tokens-allowlist.json` has one entry per offending file, with `max`, the count today.
  - The check fails when a file goes over its `max`, when an allowlisted file is clean (remove its entry), or when an allowlisted file no longer exists.
  - It prints a note when a count drops, so you can lower `max`.
  - The checker also scans `apps/desktop/coucou/src/**`. `coucou/src/settings/**` is the one pending entry: it is not scanned until it moves into Next Agent Settings.
  - A few Coucou files are listed because they hold identity colours as data (see "Coucou's windows").
  - **Coucou settings: migrated.** Its settings window (`coucou/src/settings/*`) and the hub's settings pages (Flow settings, styles, voice) are now Settings → Overlay (`routes/(app)/settings/overlay/*`), built from SettingsLayout, SettingsSection and SettingsRow; the old code stays in the tree, unused ([[Coucou]]).
- **Dark-theme parity:** the two dark blocks in `tokens.css` must match.
- **Run it:** `node scripts/check-design-tokens.mjs --list` prints every finding. Tests: `scripts/tests/design-tokens.test.ts`.

## Coucou's windows

The island, the dictation pill, the desktop orb and the hub (the vendored Coucou UI, ADR-0011) use these tokens too. Their CSS keeps its selectors, sizes, positions and animation timings; only colour, type and surface styling moved.

- `routes/coucou/+layout.svelte` loads `tokens.css` and forces `data-theme="dark"`. These windows are fixed dark surfaces (the island sits on the notch, the pill on the screen edge), so they use the dark tokens whatever the OS theme is.
- Coucou's variable names stay as aliases of tokens (`--ink`, `--dim`, `--card`, `--hub-*`, `--font` …), so a re-sync of its CSS is a plain diff. Each changed file carries a `Next Agent:` comment.
- Where Coucou had a translucent white wash (`rgba(255,255,255,0.06)`), the code uses `--color-fill-N`. A tinted wash (`rgba(47,140,255,0.14)`) is `color-mix(in srgb, var(--color-accent) 14%, transparent)`.
- Radii and `transition` times use a token only when the value is exactly the token's, so no shape or timing changes. `animation` times stay raw: the duration tokens drop to about 0 under reduced motion, which would turn a looping spinner into a flicker.
- Canvas code (Mochi's orb, the drop sequence, the share image) cannot use CSS variables. Its colours, and the per-agent and per-pill identity colours, are data: they are persisted, alpha-suffixed and drawn on canvas. Those files are on the allowlist (`pills.ts`, `agents.ts`, `hero.ts`, `integrations.ts`) until the owner's own palette replaces them.

| Coucou | Token |
|---|---|
| `#000` island, `body.hub-window` | `--color-island` (island), `--color-bg` (hub window) |
| `#f5f6f8`, `#f1f2f4`, `#e8e9ec`, `#c5c8cd` … `#d5d7db` (text) | `--color-text` |
| `#b0b5be` … `#b9bdc4`, `#9398a1`, `#8e939c`, `#8a8f98` (secondary text) | `--color-text-secondary` |
| `#7b8089`, `#6b7079`, `#5c6370` | `--color-text-tertiary` |
| `#5f646d`, `#4d5159`, `#4b5563`, `#454850` (text) | `--color-text-disabled` |
| `#5a5f68`, `#4d5159` (borders, rules) | `--color-border-strong` |
| `#0e0f11`, `#0b0c0e`, `#0d0e12`, `#020203` (backgrounds) | `--color-bg` |
| `#141518`, `#16171b` | `--color-surface-sunken` |
| `#18191c`, `#1d1f23` | `--color-surface` |
| `#1f2024` | `--color-surface-raised` |
| `#252830` | `--color-surface-overlay` |
| `#fff` button or thumb, `#000` text on it | `--color-surface-inverse`, `--color-text-inverse` |
| `#0b0c0e`, `#000` text on a light chip | `--color-text-inverse` |
| `#7c818a` (shimmer) | `--color-neutral` |
| `rgba(255,255,255,0.035 … 0.05)` | `--color-fill-1` |
| `rgba(255,255,255,0.06 … 0.08)` | `--color-fill-2` |
| `rgba(255,255,255,0.09 … 0.12)` | `--color-fill-3` |
| `rgba(255,255,255,0.13 … 0.2)` | `--color-fill-4` |
| `rgba(255,255,255,0.3 … 0.35)` | `--color-fill-5` |
| other `rgba(255,255,255,a)` (0.4, 0.75, 0.85, 0.94), `rgba(245,246,248,a)` | `color-mix(in srgb, var(--color-text) a%, transparent)` |
| `rgba(0,0,0,a)` (shadows, scrims, masks, `#000` masks) | `color-mix(in srgb, var(--color-island) a%, transparent)`, `var(--color-island)` |
| `#fb923c` hub accent, `#2f8cff`, `#6366f1`, `#7c5cff`, `#a78bfa` (fills, strokes) | `--color-accent` |
| `#fdab63` (hub primary hover) | `--color-accent-hover` |
| `#818cf8`, `#2f8cff` (text) | `--color-accent-text` |
| `#170c04` (text on the hub accent), `#fff` on `#2f8cff` | `--color-on-accent` |
| `#6e56cf` (hub focus ring) | `--color-focus` |
| `#60a5fa`, `#93c5fd` (text), `rgba(59,130,246,a)` | `--color-info-text`, `--color-info` |
| `#22d3ee`, `#38bdf8` | `--color-info` |
| `#22c55e`, `#34d399` (fills), `#2dd4a7`, `#1fa87a`, `#1ed760`, `rgba(34,197,94,a)`, `rgba(52,211,153,a)` | `--color-ok` |
| `#34d399`, `#86efac` (text) | `--color-ok-text` |
| `#f4505e`, `#ff453a`, `#ff6b5b`, `rgba(255,69,58,a)`, `rgba(244,80,94,a)` | `--color-danger` |
| `#ff8d97`, `#ff8a80`, `#fca5a5` | `--color-danger-text` |
| `#f5a524`, `#f7b32b`, `rgba(247,179,43,a)`, `rgba(245,165,36,a)`, `rgba(249,115,22,a)` | `--color-warn` |
| `#f5d28a`, `#c9956a` | `--color-warn-text` |
| `#f472b6` (unused alias `--pink`) | `--color-brand` |
| Font stacks `--font`, `--mono`, pill `system-ui, "Segoe UI" …` | `--font-sans`, `--font-mono` |
| `border-radius` 3 / 4 / 6 / 10 / 14 / 999 px | `--radius-xs / sm / md / lg / xl / full` |
| `transition` 0.06 / 0.12 / 0.2 / 0.32 s | `--duration-instant / fast / normal / slow` |

Hue shifts to expect: the hub's orange accent is the system's blue-indigo, the island's blue (`#2f8cff`) is the accent, and every grey is the nearest step of the text, surface or border scale.

## Moving a page or an upstream onto the system

1. Replace its controls with `lib/ds` components, and settings forms with SettingsLayout, SettingsSection and SettingsRow.
2. Replace raw values with tokens. If none fits, add a semantic token to `tokens.css` (light and both dark blocks) and show it on the style-guide page.
3. Run `node scripts/check-design-tokens.mjs`. Remove the file's allowlist entry when it is clean.
4. For vendored upstream code, mark each change `Next Agent:` (ADR-0011), so a re-sync stays a plain diff.
