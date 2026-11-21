---
title: Design Spec v0.2 Plan
tags: [plan, design, ui]
status: proposed
---

# Design Spec v0.2 Plan

How Next Agent adopts the owner's [[Design Spec v0.2]] (`docs/design/Design Spec v0.2.md`). The tooling choices (no Storybook, Radix or Tailwind; bundled fonts; CodeMirror 6) are in [[ADR-0014 Design Spec v0.2 Without Storybook, Radix or Tailwind]]. The rule from [[ADR-0012 One Design System]] still holds: every screen, Coucou's included, uses `lib/ds`.

**Goal:** people can download, install and use Next Agent on macOS, Windows and Linux, and it looks and works like the spec. That means:
- it works offline, with no CDN;
- it is usable without the island, the notch or any upstream;
- it is keyboard-complete;
- it meets WCAG 2.2 AA.

## Today (surveyed 2026-11-18)

| Spec area | What we have | Gap |
|---|---|---|
| Tokens | `lib/ds/tokens.css`: neutral grey, one blue accent, light is the default, system fonts | Spec colours, the agent and orchestration accents, Inter and JetBrains Mono, z-index scale, density sizes |
| Shell | `(app)/+layout.svelte`: sidebar, main, `StatusBar`; a drawer below 640 px | Title/command bar, activity rail, inspector, bottom panel, resizing, focus mode, spec breakpoints |
| Chat | `(app)/+page.svelte`, `Timeline`, `ToolCard`, `ApprovalCard`, `RightPanel` | Summary → Plan → Actions → Verification → Details layout, context chips, full state list |
| Agents | Tasks in the sidebar, the task timeline, worktrees, cron | Agents mode: run list and graph, a 7-tab inspector, run controls |
| Knowledge | `notes` (pages, backlinks), `boards` | Notes and boards as one mode, block references to code and runs, an "exposed to agents" control |
| Design | `design` (Design Studio), Settings → Design system | Token browser and component catalogue as the Design mode |
| Code | `DiffViewer` only | Editor (CodeMirror 6), file tree, tabs, a terminal in the bottom panel |
| Source control | Worktree merge and review in tasks | Changes, staging, commit, history, conflicts |
| Monitor | `usage`, Settings → Diagnostics | One Monitor mode: event stream, filters, routing and cost panels |
| Task state | Rust `TaskStatus`: `Pending`, `Running`, `Completed`, `Failed`, `Cancelled` | `queued`, `blocked`, `awaiting_approval`, `timed_out`, `unknown`, plus `updatedAt`, `source` and `reason` |
| Catalogue and tests | Living style guide, `node --test`, svelte-check | Playwright, axe, visual snapshots |

Components the spec lists that we have, and the name we keep:
- `StatusIndicator` = `StatusDot`
- `ToolCallCard` = `ToolCard`
- `ApprovalDialog` = `ApprovalCard`
- `DiffViewer`
- `LogEventRow` = the Diagnostics table row
- `TokenSwatch` and `ComponentPreview` = the style guide

New components: `AppShell`, `ActivityRail`, `ProjectSwitcher`, `CommandPalette`, `ResizablePanel`, `Menu`/`Popover`, `Tree`, `EditorTab`, `TaskCard`, `TaskGraph`, `KnowledgeBlock`, `GitChangeItem`, `ModelRouteBadge`, `Toast`.

## Interfaces (fixed here so agents can work in parallel)

### Tokens
- We keep our names. The spec's names map onto them; we add only what's missing.

| Spec name | Ours |
|---|---|
| `--color-bg-canvas` | `--color-bg` |
| `--color-bg-surface` | `--color-surface` (panels) and `--color-surface-sunken` (sidebars) |
| `--color-bg-elevated` | `--color-surface-raised` and `--color-surface-overlay` |
| `--color-bg-hover` | `--color-surface-hover` |
| `--color-border-subtle` / `-default` | `--color-border-subtle` / `--color-border` |
| `--color-text-primary` / `-secondary` / `-muted` | `--color-text` / `--color-text-secondary` / `--color-text-tertiary` |
| `--color-accent-primary` | `--color-accent` |
| `--color-status-*` | `--color-ok`, `--color-warn`, `--color-danger`, `--color-info` |
| `--color-focus-ring` | `--color-focus` |

- **New tokens:**
  - `--color-accent-agent`, `--color-accent-orchestration`, each with `-subtle` and `-text` variants;
  - `--z-base`, `--z-sticky`, `--z-dropdown`, `--z-popover`, `--z-modal`, `--z-toast`, `--z-tooltip`;
  - `--control-height`, `--row-height`, `--tab-height`, `--toolbar-height` (comfortable; `[data-density="compact"]` overrides them);
  - `--shell-titlebar` 44px, `--shell-rail` 56px, `--shell-sidebar` 248px, `--shell-inspector` 320px, `--shell-bottom` 220px, `--shell-statusbar` 28px;
  - `--font-sans` with Inter first, `--font-mono` with JetBrains Mono first.
- **Values:** take the spec's hex values for light and dark. Brand stays the logo's (`--color-brand-*`), not the spec's `#2563EB`. Existing type sizes move to the spec's scale (12/13/14/16/18/22/28). The base size stays 14.
- **Radii:** sm 4, md 8, lg 12, xl 16. We keep `--radius-xs` 3 for badges.

### Task state (Rust → TS contract)
```ts
type TaskState = 'draft' | 'queued' | 'blocked' | 'running' | 'awaiting_approval'
  | 'succeeded' | 'failed' | 'cancelled' | 'timed_out' | 'unknown';
type TaskStatus = { state: TaskState; updatedAt: string; source: string; reason?: string };
```
- Rust gets a matching enum, and a migration maps old rows: `Pending` → `queued`, `Completed` → `succeeded`.
- `reason` is free text that can name user content, so it follows the logging rules in [[ADR-0013 Local Diagnostics Log Without User Content]]: it is never logged.
- The UI never shows a percentage unless a real total exists.

### Shell
- `AppShell` props:
  - `mode`
  - `sidebar` (snippet)
  - `inspector?` (snippet), with `inspectorTabs`
  - `bottom?` (snippet)
  - `statusbar` (snippet)
- `lib/shell.svelte.ts` keeps the session state:
  - the panel widths and which panels are visible, which persist (non-sensitive);
  - the per-mode selection and scroll, which last for the session only;
  - `focusMode`.
- **Modes and routes:**

| Mode | Route | Notes |
|---|---|---|
| Chat | `/` | |
| Code | `/code` | |
| Agents | `/agents` | |
| Knowledge | `/knowledge` | `notes` and `boards` become its tabs; the old URLs redirect |
| Design | `/design` | Design Studio plus the catalogue tab |
| Source Control | `/git` | |
| Monitor | `/monitor` | Usage and Diagnostics as tabs; Settings → Diagnostics keeps the level and export controls |

- `CommandPalette` commands are `{ id, label, mode?, run, sideEffect: boolean, risk?: 'low'|'high' }`. Commands with side effects show in their own group with a preview line.

## Work breakdown

### How the work runs
- **One agent per task**, each in a worktree. The coordinator merges after each wave and runs everything:
  - `cargo test --workspace`, `make lint`, `make check`, `node scripts/test.mjs`;
  - svelte-check and the build;
  - from Wave 1 on, `pnpm -C apps/desktop e2e`.

  Then it deletes the finished agents and worktrees.
- **Models:** Opus for contracts, security or data migrations, and destructive git actions. Sonnet for UI and judgement work. Haiku for mechanical moves.
- **Skills, named in every brief:**
  - `simplify` at the end of each task;
  - `engineering:code-review` before handing back;
  - `anthropic-skills:built-in-browser` for visual checks of the dev server;
  - `engineering:testing-strategy` for Wave 1-B;
  - `engineering:debug` when something breaks.
- **Every task's done-when includes:**
  - the token check passes, with no new allowlist entries;
  - keyboard-only use of the new UI works;
  - light and dark screenshots are in the report.

### Wave 1: foundations (4 agents in parallel)
| # | Task | Model | Est. | Owns | Done when |
|---|---|---|---|---|---|
| 1-A | **Tokens and fonts:** spec values for light and dark; the new tokens above; bundled Inter and JetBrains Mono woff2 in `static/fonts/` with `@font-face` in `tokens.css`; OFL text in `NOTICE.md`; regenerate the root `tokens.css` and `DESIGN.md` from the Design Studio; update the style guide and the [[Design System]] doc | Sonnet | 3 h | `lib/ds/tokens.css`, `static/fonts/**`, `NOTICE.md`, root `tokens.css` and `DESIGN.md`, `docs/design/Design System.md`, the style guide page | The style guide shows the spec palette; the contrast table in the doc passes AA in both themes; fonts load offline with the CSP unchanged |
| 1-B | **Test harness:** Playwright with `@axe-core/playwright` (dev deps), running against `vite preview` with the mock API; snapshots of the style guide and every route at 800, 1100, 1440 and 1920 px in light and dark; `pnpm -C apps/desktop e2e`; a macOS CI job | Sonnet | 3 h | `apps/desktop/e2e/**`, `playwright.config.ts`, `package.json` scripts, `.github/workflows/ci.yml` | It runs green on today's UI, which is the baseline; axe violations are listed in the report, not fixed |
| 1-C | **New primitives:** `ResizablePanel` (a focusable separator, arrow keys, `aria-valuenow`); `Menu` and `Popover` (APG menu button); `Tree` (APG tree view); `Toast` (with a persistent notification list); `CommandPalette` (APG combobox, ⌘K/Ctrl+K, side-effect group). Each goes in `lib/ds` with stories on the style guide page | Sonnet | 6 h | `lib/ds/{ResizablePanel,Menu,Popover,Tree,Toast,CommandPalette}.svelte`, `lib/ds/index.ts`, the style guide page | Each one has every state from spec §4.1 on the style guide, node tests for its keyboard logic, and no axe violations |
| 1-D | **Task state contract:** new Rust enum and SQL migration; `updated_at`, `source` and `reason` columns; TS contract codegen; `awaiting_approval` set by the broker while an approval is pending; `timed_out` for tasks over their limit; `unknown` when the process manager loses a child | Opus | 4 h | `crates/core/src/task.rs`, the store migration, `crates/core/src/broker.rs` (state hooks only), `packages/contracts/**`, `apps/desktop/src/lib/api.ts` types | Old databases migrate (with a test); every state has a test; terminal states are as in spec §4.2; no UI changes |

### Wave 2: the shell (2 agents in parallel, after Wave 1)
| # | Task | Model | Est. | Owns | Done when |
|---|---|---|---|---|---|
| 2-A | **AppShell:** title bar (mark, project switcher, repo and branch, ⌘K, notifications, settings); `ActivityRail` with the 7 modes; resizable sidebar, inspector and bottom panel; status bar (connection, model route, branch, task, approvals, cost only when known); focus mode; breakpoints from spec §5; mode routes as stubs that link to the existing pages | Sonnet | 1 day | `(app)/+layout.svelte`, `lib/components/{Sidebar,StatusBar,RightPanel}.svelte`, `lib/ds/{AppShell,ActivityRail,ProjectSwitcher}.svelte`, `lib/shell.svelte.ts`, new route folders (stubs only) | Every current page still works inside the shell; panel widths persist; Esc closes dialogs without leaving focus mode; snapshots at the 4 widths are updated |
| 2-B | **Command palette content:** a registry of navigation, project and AI commands from every page; risky commands show a preview and go through the approval flow | Sonnet | 3 h | `lib/commands.ts`, registration calls in each page (one line each) | Every mode can be reached from ⌘K; commands with side effects are listed apart from the rest; there are node tests for the registry |

### Wave 3: modes (5 agents in parallel, after Wave 2)
| # | Task | Model | Est. | Owns |
|---|---|---|---|---|
| 3-A | **Chat:** the response layout Summary → Plan → Actions → Verification → Details; `TaskCard`; context chips with where each came from; every state in spec §2.1; Stop acknowledges real cancellation | Sonnet | 1 day | `(app)/+page.svelte`, `Timeline`, `ToolCard`, `ApprovalCard`, `lib/ds/TaskCard.svelte` |
| 3-B | **Agents:** the run list (required) and `TaskGraph` (an optional view, with only the explicit dependency edges); an inspector with Overview, Steps, Tool calls, Artifacts, Logs, Approvals and Usage tabs; controls only where supported, with a tooltip saying why when one is off | Sonnet | 1 day | `(app)/agents/**`, `lib/ds/TaskGraph.svelte` |
| 3-C | **Knowledge:** notes and boards as tabs of one mode; `KnowledgeBlock`; block references to tasks and runs; a per-page "share with agents" switch (default in owner decision 3); redirects from the old URLs | Sonnet | 1 day | `(app)/knowledge/**`, the moved `notes` and `boards` pages, `lib/notes/**` (UI only) |
| 3-D | **Monitor:** Usage and Diagnostics events as one stream with filters (project, run, agent, tool, status, severity, time, provider); a routing panel; a cost panel that tells measured, estimated and unavailable apart | Sonnet | 6 h | `(app)/monitor/**`, the moved `usage` page; the Diagnostics settings keep only the level and export controls |
| 3-E | **Design mode catalogue:** the style guide becomes a Design-mode tab (token browser with contrast samples, components with the state matrix, theme, density and width switches); Settings → Design system redirects there | Haiku | 3 h | `(app)/design/catalogue/**`, the moved style guide page |

### Wave 4: Code and Source Control (2 agents, after Wave 3)
| # | Task | Model | Est. | Owns |
|---|---|---|---|---|
| 4-A | **Code mode:** CodeMirror 6 editor, `EditorTab`, a file tree (`Tree`), file status labels, external-change protection, AI suggestions shown apart from saved files; a terminal in the bottom panel only through the existing process manager and permission engine | Sonnet | 2 days | `(app)/code/**`, `lib/code/**`, new read and write commands in `src-tauri` that go through the permission engine |
| 4-B | **Source control:** changes grouped into staged and unstaged, stage by file and hunk, commit, history, conflict choices with a backup first; agent changes labelled with their run; reset, discard and force push behind risk wording and a confirm, offering a safer option | Opus | 2 days | `(app)/git/**`, `lib/ds/GitChangeItem.svelte`, a `crates/core` git module, `src-tauri` git commands |

### Wave 5: hardening and installing (3 agents, after Wave 4)
| # | Task | Model | Est. | Owns |
|---|---|---|---|---|
| 5-A | **Accessibility pass:** fix every axe violation; screen-reader live regions for completions, errors and approvals (never one per log line); focus return on every overlay; reduced motion; 200% text size | Sonnet | 1 day | Any UI file; no behaviour changes |
| 5-B | **First run and install:** works with no upstreams, no keys, no notch and no Coucou; an onboarding empty state per mode; Windows and Linux builds in CI produce an `.msi` and an `.AppImage`/`.deb`; macOS `.dmg`; a release checklist in `docs/guides/` | Opus | 1 day | `.github/workflows/**`, `tauri.conf.json` bundle settings, `docs/guides/Install.md`, empty states |
| 5-C | **Docs:** the [[Design System]] guide gets the new components, shell, density and modes; screenshots; `README.md` install section | Haiku | 3 h | `docs/design/**`, `README.md` |

**Total:** about 12 agent-days, run as 5 waves. When waves run in parallel, it takes about 6–7 working days of wall-clock time, plus the coordinator's merges.

## Owner decisions (defaults used unless changed)
1. **Default theme:** the spec says dark first. **Default:** follow the OS, with the spec's dark values. A downloaded app should match the user's system.
2. **Brand blue:** the spec says `#2563EB`; the logo uses `#136AEF`. **Default:** the logo colours stay the brand, and the spec's blues are used for the interactive accent only.
3. **Knowledge shared with agents:** **Default:** new pages are shared, existing pages stay as they behave today, and each page has a switch.
4. **Compact density:** **Default:** shipped, but off by default.
5. **Signing:** macOS notarising and Windows signing need paid certificates. **Default:** unsigned builds, with install steps in the guide, until you get certificates.

## Out of scope
- A web build: the spec lists it as optional.
- Team roles and permissions: single-user only for now.
- New upstream adapters (OpenCode, Hermes, Open Crew, 9Router). The shell only shows what each existing adapter reports, and shows `Unavailable` otherwise.
