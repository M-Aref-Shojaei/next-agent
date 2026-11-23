# Coding Agent — Design System v0.2

**Status:** Implementation-ready specification
**Product:** Adaptive-hybrid AI coding workspace
**Primary platform:** macOS desktop; responsive for Windows, Linux, and web
**Version:** 0.2
**Audience:** Claude Code and human designers/developers

> **Objective:** Build a unified developer workspace that combines AI-native design tools, knowledge management (Logseq-style), agent orchestration (Hermes Agent / Open Crew), coding (OpenCode), desktop awareness (Coucou), routing (9Router), and source control. These names describe integration surfaces, not permission to copy other products' branding or proprietary UIs. Confirm each integration's actual available API and capabilities before implementation.

## 0. Non-negotiable principles

1. **Adaptive hybrid:** Users can move between Chat, Code, Agents, Knowledge, Design, and Operations without changing project context.
2. **One workspace, many views:** Shared project, repository, task, agent, and artifact state across modes.
3. **Summary first:** Prioritize outcomes, next actions, and concise status. Logs, tool calls, and reasoning *summaries* are expandable. Never presume private chain-of-thought access.
4. **Observable autonomy:** Display task ownership, agent/tool activity, elapsed time, failures, approval gates, and outputs.
5. **Human control:** Permission and approval UX for write, execute, network, git, deployment, and destructive actions.
6. **Reversible operations:** Show diffs before risky writes; support checkpoints, undo where feasible, and recovery through Git.
7. **Quiet by default:** Minimal interruptions, strong keyboard support, accessible contrast, and restrained motion.
8. **Provider-neutral:** Model routing is a capability, not the application's visual identity.
9. **Truthful UI:** Distinguish queued/running/succeeded/failed/cancelled; never show fabricated progress, costs, model capabilities, or test results.

## 1. Information architecture

### Persistent shell

- **Title / command bar (44 px):** Product mark, project/workspace switcher, current repository/branch, global search/command palette, notifications, user/settings.
- **Activity rail (56 px):** Chat, Code, Agents, Knowledge, Design, Source Control, Monitor. Can collapse to icons. Active item indicated via accent rail and accessible label.
- **Left contextual sidebar (default 248 px, 200–360 px):** Project-specific tree/list/filter. Can hide.
- **Main content region (flex, min 480 px where feasible):** Mode-specific work surface.
- **Right inspector (default 320 px, 280–480 px):** Context, selected artifact, agent details, diffs or properties. Can hide, resize or switch tabs.
- **Bottom panel (default 220 px, min 120 px):** Terminal, logs, problems, test results, events. Collapsible, resizable.
- **Status bar (28 px):** Connection health, model/provider, git branch, current task, approvals, budget/cost indicator when known.

### Modes and entry points

| Mode | Primary job | Main surface | Contextual sidebar | Inspector |
|---|---|---|---|---|
| Chat | Specify outcomes, delegate, review | Conversation + task plan | Conversations / tasks | Context, artifacts, approvals |
| Code | Edit, run, inspect | Editor tabs / diffs | File tree / symbols | Agent assistance / tests |
| Agents | Supervise orchestration | Task graph / executions | Runs / crews | Agent details / traces |
| Knowledge | Capture/retrieve project context | Notes + backlinks / graph | Pages / tags | Sources / links |
| Design | Inspect tokens and components | Component canvas + previews | Component tree | Props / tokens / states |
| Source Control | Manage change sets | Changes / commit / history | Repositories / branches | Diff / conflict details |
| Monitor | Diagnose and optimize | Events, logs, usage | Filters / sessions | Selected event / telemetry |

### Adaptive layout behaviors

- Preserve open files, selected task, scroll position, and inspector tabs on mode changes **within the session**; persist only non-sensitive preferences and supported workspace state.
- Selecting a file from Chat or Agents opens it in Code, with a back action returning to the originating context.
- Selecting a cited note opens Knowledge at the relevant block and retains source reference.
- Selecting a tool error opens Monitor filtered to that run/event.
- Command palette (`Cmd/Ctrl+K`) offers navigation, project actions, and contextual AI commands; clearly separate commands that take side effects.
- An optional **Focus Mode** hides both sidebars and bottom panel; `Esc` exits dialogs, not the entire focus mode unexpectedly.
- Use multi-panel split views only where the viewport supports them; never squeeze editors below usable widths.

## 2. Screen specifications

### 2.1 Chat / Orchestrator

**Default layout:** Conversation in main region; task outline on left; context and approval queue on right.

Sections, top to bottom:
1. Session header: title, repository, branch, model route (if known), run status, overflow actions.
2. Message timeline: user, assistant, tool calls, attachments, cited sources, execution results.
3. AI response: **Summary → Plan → Actions → Verification → Details**. Collapsible technical details.
4. Task execution cards: owner, state, dependencies, current step, last event, actions.
5. Composer: multiline input, mention files/notes/agents, attach context, model route selector, permission mode, send/stop.

**States:** Empty (suggest first tasks); composing; submitting; streaming; waiting for tool; awaiting approval; completed; failed; interrupted; disconnected. Streaming messages must not imply execution completed. Stop should request cancellation and then acknowledge actual cancellation status.

**Composer behavior:** `Enter` sends, `Shift+Enter` newline where user preference allows; support IME; mention suggestions keyboard accessible. Show context attachment chips with removal and provenance. Do not send hidden files automatically.

### 2.2 Code / Editor

**Default layout:** Files + symbols in left sidebar; tabbed editor/diff in main; AI assistance / diagnostics in inspector; terminal/tests in bottom panel.

Required:
- Tab bar with dirty marker, close action, and overflow.
- Code editor using an existing editor engine (e.g. Monaco or CodeMirror 6); this design system does **not** reimplement code editing primitives.
- File status annotations (`modified`, `new`, `deleted`, `conflicted`, `read-only`).
- Inline diagnostics and quick fixes tied to actual tooling.
- Split diff with accept/reject hunks, file-level review, and protection against overwriting external changes.
- Run/test results show authoritative command, exit code, timestamp and captured output.
- AI code suggestions are visibly distinct from saved filesystem state.

**States:** No file, loading, editable, read-only, dirty, conflict, external modification, save failure, large file, unsupported file, agent proposing change, reviewing diff.

### 2.3 Agents / Orchestration

**Default layout:** Toolbar + filtered list/graph toggle; center task graph; right run inspector.

Task representation:
- Each node: id, concise task title, owner/agent, state, dependencies, last update, relevant artifact links.
- Graph edges represent explicit dependencies, never inferred causal links.
- List view is mandatory for keyboard users and dense workflows; graph is optional enhancement.
- Inspector tabs: Overview, Steps, Tool Calls, Artifacts, Logs, Approvals, Usage.
- Visual pipeline: **Plan → Inspect → Edit → Fix → Test Authoring → Test Execution → Review → Report**. Treat as a configurable workflow, not a guarantee every task requires each step.

Run controls: Pause (only when supported), Resume, Stop, Retry from checkpoint, Reassign, Open artifact. Disable unsupported actions with explanatory tooltip.

**States:** Draft, queued, blocked, running, awaiting approval, succeeded, failed, cancelled, timed out, stale/unknown. Provide stale status when connection cannot confirm current state.

### 2.4 Knowledge / Logseq-style

**Default layout:** Page tree and search on left; block-based note editor main; backlinks and source provenance right.

Required patterns:
- Hierarchical blocks with stable IDs and keyboard indentation.
- Bidirectional links and backlinks; tags, project/entity references, and full-text search.
- Block references to tasks, code ranges, commits and run results, with source validity indicators.
- Graph view optional; always provide list/search alternative.
- Show source type, last indexed time, and extraction confidence **only if measured**.
- Explicit controls for what knowledge is exposed to agents and what remains private.

**States:** New page, indexing, ready, broken link, conflicting edit, offline cache, permissions denied, stale index, empty results.

### 2.5 Design System / AI-assisted UI

**Default layout:** Component navigation left; canvas / story previews main; properties and tokens right.

Required:
- Token browser (colors, type, spacing, sizing, radii, borders, motion, z-index).
- Component catalog with variants, interactive states, responsiveness and accessibility documentation.
- Preview matrix for light/dark themes, density, and screen widths.
- Code/spec view with copy affordance; show generated code as proposed until reviewed.
- AI design actions attach traceable requests and generated diffs; do not silently overwrite component source.

**States:** No component selected, preview loading, valid, invalid props, generation pending, candidate ready, validation failed, publishing pending, published.

### 2.6 Source Control

**Default layout:** Repo/branch controls and staged/unstaged file groups; diffs and history in main; details in inspector.

Required:
- Branch indicator; ahead/behind (when fetched), clean/dirty, detached HEAD warnings.
- Stage/unstage by file/hunk when backend supports it.
- Commit message composer and confirmation of commit result.
- Conflict editor with explicit choices and non-destructive backups/checkpoints.
- Agent-generated change sets visibly attributed to run/task.
- Destructive operations (reset hard, discard unsaved changes, force push) have explicit risk language and confirmation. Prefer safer alternatives.

**States:** No repository, clean, dirty, staging, committing, fetching, conflict, auth failure, offline, detached HEAD, protected branch.

### 2.7 Monitor / Routing

**Default layout:** Overview cards on top; events/log stream central; route/cost and run inspection to side.

Required:
- Filters: project, session, run, agent, tool, status, severity, time window, provider.
- Event viewer with timestamp, correlation ID, level, source, payload preview, expandable structured fields.
- Routing panel: configured models, chosen route, fallback event, provider error, latency and context usage **when actually reported**.
- Cost panel distinguishes measured charges, provider-reported estimates, and unavailable data.
- Avoid exposing credentials, full prompts, or sensitive environment data in default logs; redact and permission-gate raw inspection.

**States:** Live, paused stream, backfilling, no data, filter-empty, stale, disconnected, insufficient permission.

## 3. Design tokens

Tokens below are initial values, not imported from any third-party design system. Store as semantic CSS variables and typed JSON/TS tokens. Use semantic tokens in components; do not hardcode palette values in feature code.

### 3.1 Color — dark theme (default)

| Token | Hex | Usage |
|---|---|---|
| `--color-bg-canvas` | `#0B1020` | Root canvas |
| `--color-bg-surface` | `#151D30` | Sidebar and panels |
| `--color-bg-elevated` | `#202B40` | Popovers, selected surfaces |
| `--color-bg-hover` | `#293750` | Hover states |
| `--color-border-subtle` | `#263249` | Dividers |
| `--color-border-default` | `#35425C` | Controls and panel edges |
| `--color-text-primary` | `#F2F5FB` | Primary content |
| `--color-text-secondary` | `#B5C1D6` | Secondary content |
| `--color-text-muted` | `#8898B1` | Non-critical hints (verify contrast) |
| `--color-accent-primary` | `#60A5FA` | Primary interactive elements on dark |
| `--color-accent-agent` | `#2DD4BF` | Agent activity |
| `--color-accent-orchestration` | `#A78BFA` | Knowledge / orchestration |
| `--color-status-success` | `#34D399` | Confirmed success |
| `--color-status-warning` | `#FBBF24` | Caution / awaiting action |
| `--color-status-error` | `#FB7185` | Errors / destructive intent |
| `--color-status-info` | `#60A5FA` | Informational state |
| `--color-focus-ring` | `#93C5FD` | Visible keyboard focus |

Brand colors for illustration/identity: electric blue `#2563EB`, teal `#14B8A6`, violet `#8054E8`. **Do not use brand colors directly for normal body text without contrast checks.**

### 3.2 Color — light theme

| Token | Hex |
|---|---|
| `--color-bg-canvas` | `#F5F7FB` |
| `--color-bg-surface` | `#FFFFFF` |
| `--color-bg-elevated` | `#EDF2F9` |
| `--color-bg-hover` | `#E5ECF6` |
| `--color-border-subtle` | `#DFE5EF` |
| `--color-border-default` | `#BCC8D9` |
| `--color-text-primary` | `#142033` |
| `--color-text-secondary` | `#475569` |
| `--color-text-muted` | `#64748B` |
| `--color-accent-primary` | `#1D4ED8` |
| `--color-accent-agent` | `#0F766E` |
| `--color-accent-orchestration` | `#6D28D9` |
| `--color-status-success` | `#047857` |
| `--color-status-warning` | `#92400E` |
| `--color-status-error` | `#BE123C` |
| `--color-status-info` | `#1D4ED8` |
| `--color-focus-ring` | `#2563EB` |

### 3.3 Typography

- UI font: `Inter`, fallback `system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`.
- Monospace: `JetBrains Mono`, fallback `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`.
- Font weights: 400 regular, 500 medium, 600 semibold, 700 bold (rare).
- Sizes / line heights: `xs 12/16`, `sm 13/18`, `base 14/20`, `md 16/24`, `lg 18/26`, `xl 22/30`, `2xl 28/36` px.
- Default UI text: 14/20; dense table: 13/18; code: 13/20; user-adjustable font scaling.
- Avoid typography smaller than 12 px except carefully vetted non-essential metadata. Respect OS zoom and text scaling.

### 3.4 Spacing, shape, elevation

- Spacing base: **4 px**. Scale: `0:0`, `1:4`, `2:8`, `3:12`, `4:16`, `5:20`, `6:24`, `8:32`, `10:40`, `12:48`, `16:64` px.
- Radii: `sm 4`, `md 8`, `lg 12`, `xl 16`, `pill 9999` px.
- Border: 1 px default; 2 px focus outline (do not replace focus solely with color).
- Shadow (dark): prefer border + subtle elevation; no large diffuse shadows in the base layout.
- Icons: 16 px in dense controls, 20 px default, 24 px feature-level; stroke 1.75–2 px.
- Touch/click targets: aim 36–40 px minimum for desktop controls and 44 px for touch-targeted layouts; provide spacing to avoid accidental clicks.
- Motion: 120 ms hover, 180 ms open/close, 240 ms layout change; honor `prefers-reduced-motion` and avoid animated spinners as the sole state indicator.
- Z layers: `base:0`, `sticky:10`, `dropdown:30`, `popover:40`, `modal:60`, `toast:80`, `tooltip:90`.

### 3.5 Density and sizes

| Element | Comfortable | Compact |
|---|---:|---:|
| Standard control height | 36 px | 30 px |
| Row height | 40 px | 32 px |
| Sidebar item | 36 px | 30 px |
| Tab height | 36 px | 32 px |
| Main toolbar height | 44 px | 40 px |
| Dialog max width | 560 px | 560 px |

Compact mode is optional and must not compromise keyboard accessibility.

### 3.6 Example CSS primitives

```css
:root[data-theme="dark"] {
  color-scheme: dark;
  --color-bg-canvas: #0B1020;
  --color-bg-surface: #151D30;
  --color-bg-elevated: #202B40;
  --color-bg-hover: #293750;
  --color-border-subtle: #263249;
  --color-border-default: #35425C;
  --color-text-primary: #F2F5FB;
  --color-text-secondary: #B5C1D6;
  --color-text-muted: #8898B1;
  --color-accent-primary: #60A5FA;
  --color-accent-agent: #2DD4BF;
  --color-accent-orchestration: #A78BFA;
  --color-status-success: #34D399;
  --color-status-warning: #FBBF24;
  --color-status-error: #FB7185;
  --color-status-info: #60A5FA;
  --color-focus-ring: #93C5FD;
  --space-unit: 4px;
  --radius-sm: 4px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
}

:root[data-theme="light"] {
  color-scheme: light;
  --color-bg-canvas: #F5F7FB;
  --color-bg-surface: #FFFFFF;
  --color-bg-elevated: #EDF2F9;
  --color-bg-hover: #E5ECF6;
  --color-border-subtle: #DFE5EF;
  --color-border-default: #BCC8D9;
  --color-text-primary: #142033;
  --color-text-secondary: #475569;
  --color-text-muted: #64748B;
  --color-accent-primary: #1D4ED8;
  --color-accent-agent: #0F766E;
  --color-accent-orchestration: #6D28D9;
  --color-status-success: #047857;
  --color-status-warning: #92400E;
  --color-status-error: #BE123C;
  --color-status-info: #1D4ED8;
  --color-focus-ring: #2563EB;
}
```

## 4. Component specifications

Every component must document: API/props, variants, size, state matrix, keyboard interactions, accessibility semantics, responsive behavior, empty/loading/error behaviors, and examples in Storybook.

| Component | Variants / props | Interaction contract |
|---|---|---|
| `AppShell` | mode, left/right/bottom visibility, density | Panels resize via keyboard-accessible handles; remembers widths |
| `ActivityRail` | items, selected, badges | Arrow keys follow established nav semantics, visible selected state |
| `ProjectSwitcher` | projects, active, recent | Search, select, loading, failure |
| `CommandPalette` | commands, search, context | `Cmd/Ctrl+K` toggles, keyboard select, action preview for risky commands |
| `ResizablePanel` | min/max/default width | Focusable separator with value announcement |
| `EditorTab` | dirty, pinned, conflict, loading | Close/pin/reorder; unsaved change guard |
| `AgentMessage` | role, status, citations, artifacts | Expand source/tool detail; link to evidence |
| `AgentComposer` | attachments, route, permissions | Send/stop, keyboard shortcuts, privacy confirmation |
| `TaskCard` | state, owner, dependency, progress | Open details, retry/stop only when supported |
| `TaskGraph` | nodes, edges, selection | Zoom/pan + equivalent accessible list view |
| `ToolCallCard` | tool, args summary, outcome, duration | Expand safe details, redact sensitive arguments |
| `ApprovalDialog` | action, scope, affected files, risk | Explicit allow once / deny; persistent permissions separate |
| `DiffViewer` | file, hunks, proposed changes | Review, accept/reject with save conflict checking |
| `KnowledgeBlock` | id, text, links, depth | Keyboard nesting, backlink navigation |
| `TokenSwatch` | name, value, theme | Copy token reference; contrast sample |
| `ComponentPreview` | story, variants, viewport | State switcher; light/dark previews |
| `GitChangeItem` | path, status, staged | Stage/unstage; diff opens with provenance |
| `LogEventRow` | severity, time, run, message | Expand structured payload / jump to run |
| `ModelRouteBadge` | configured route, active fallback | Clearly mark unknown/stale data |
| `StatusIndicator` | state, label | Text + icon + color; not color alone |
| `Toast` | severity, action | Non-blocking; errors accessible via persistent log/notification center |

### 4.1 Universal interaction states

For all interactive components: `default`, `hover`, `focus-visible`, `active`, `selected` where applicable, `disabled`, `loading`, `error`, and `success` only when semantically relevant. Show selected and focused separately. Disabled controls cannot fire actions. Loading must prevent duplicate submissions or make idempotence explicit.

### 4.2 State model: agent/task

Use explicit state with a timestamp and source:

```ts
type TaskState =
  | 'draft' | 'queued' | 'blocked' | 'running'
  | 'awaiting_approval' | 'succeeded' | 'failed'
  | 'cancelled' | 'timed_out' | 'unknown';

type TaskStatus = {
  state: TaskState;
  updatedAt: string;       // ISO 8601
  source: string;          // authoritative adapter or service
  reason?: string;
};
```

Rules:
- Terminal states are `succeeded`, `failed`, `cancelled`, `timed_out`; retry creates a new attempt or explicit transition tracked in history.
- `unknown` means insufficient trustworthy status, not success or failure.
- Progress percentage is **not shown** unless a trustworthy denominator exists. Otherwise use current step + elapsed time.
- Tool calls remain attached to the originating attempt, including if the agent is reassigned.

### 4.3 Approval UX contract

Each approval must present:
- **Actor:** which agent/user requested the action.
- **Action:** edit files, run command, access network, write repository, deploy, etc.
- **Scope:** actual paths, commands, resources, affected repositories.
- **Risk:** operational impact and reversibility, not vague red labels.
- **Choices:** Deny, Allow once; optionally a separately configured limited rule with clear scope and expiry.
- **Audit:** decision, timestamp, actor, operation ID, result.

Never default to approval for destructive operations. Mask credentials in previews and logs.

## 5. Responsive and platform behavior

Breakpoints are **layout guidance**, not device-specific branding:

- `≥ 1440 px`: activity rail + left sidebar + main + optional right inspector; bottom panel supported.
- `1100–1439 px`: left sidebar + main; inspector becomes overlay or collapsible.
- `800–1099 px`: single primary surface with drawer navigation; bottom panel as tab/sheet.
- `< 800 px`: compact single-column, read/review-focused. Complex code editing and graph views may need explicit desktop recommendation; never silently remove critical approval controls.

Cross-platform:
- macOS: `Cmd`, familiar native-style panel arrangements, window safe areas.
- Windows/Linux: `Ctrl` alternatives and platform-appropriate menus.
- Screen-reader names and keyboard behavior should not depend on platform-specific iconography.
- Coucou-like OS surfaces are **optional integration affordances**; the app remains fully usable without them.

## 6. Accessibility requirements

Target **WCAG 2.2 AA** for applicable web-rendered controls and layouts.

- Text contrast at least 4.5:1 for normal text, 3:1 for large text; active controls and graphical UI boundaries at least 3:1 where required. Validate token combinations in actual contexts.
- Full keyboard navigation through all core tasks, including resizing panels and reviewing approvals.
- Visible focus indicators; do not rely on color alone for statuses, conflicts, or tool outcomes.
- Screen-reader live announcements for task completion, errors, and approvals; do not flood live regions with every log line.
- Respect reduced motion, OS scaling, and editor font preferences.
- Maintain focus correctly across overlays: focus enters modal, is trapped where appropriate, and returns to invoking control on close.
- Provide list alternatives for graphs/canvases and accessible text descriptions for agent edges and dependency state.

## 7. Reference user flows

### Flow A — Delegate implementation
1. User selects project and describes outcome in Chat.
2. UI shows concise proposed plan, affected areas, agent assignments, and required permissions.
3. User approves the plan or edits it.
4. Agents execute tasks; user can inspect steps, files, logs, and costs when available.
5. Proposed code changes open in Code diff review.
6. Tests report actual commands/results, then reviewer delivers findings.
7. User accepts changes and commits using Source Control.
8. Summary links relevant code, test evidence, and saved knowledge.

### Flow B — Investigate failure
1. Error toast links to a persistent Monitor event.
2. User inspects correlated task/run/tool events and redacted error details.
3. User opens the relevant file in Code and the responsible agent in Agents.
4. User retries from the supported step/checkpoint; previous attempt stays in history.

### Flow C — Turn knowledge into implementation context
1. User searches Knowledge and selects note blocks.
2. Selected blocks become explicit context chips in Chat.
3. Agent uses only approved references and indicates citations/provenance in output.
4. New findings are proposed as knowledge updates, not silently written.

### Flow D — AI-assisted design change
1. User selects a component in Design.
2. User requests a variant from an agent.
3. Generated code/design changes appear as a candidate with token compliance checks.
4. User previews default/hover/focus/error states and mobile widths.
5. User reviews source diff, tests, and approves merge/publish.

## 8. Recommended package architecture

Recommended only if consistent with the existing repository; inspect before introducing new libraries.

```text
packages/
  design-tokens/
    src/
      colors.ts
      spacing.ts
      typography.ts
      motion.ts
      themes.css
  ui-primitives/
    src/
      Button/
      Dialog/
      Popover/
      Tabs/
      ResizablePanel/
  ui-components/
    src/
      AppShell/
      ActivityRail/
      TaskCard/
      ToolCallCard/
      ApprovalDialog/
      DiffViewer/
  feature-chat/
  feature-code/
  feature-agents/
  feature-knowledge/
  feature-design/
  feature-source-control/
  feature-monitor/
apps/
  desktop/
  web/                     # optional
```

**Suggested UI stack:** React + TypeScript, Tailwind CSS with semantic CSS-variable tokens, accessible primitives (e.g. Radix UI), Storybook, Vitest, Testing Library, Playwright, axe-core. Use Monaco or CodeMirror for editors. Do not blindly migrate an existing stack.

### Separation of concerns

- UI components must depend on typed application interfaces, not direct vendor SDKs.
- Vendor adapters (OpenCode, Hermes, Open Crew, Logseq, 9Router, Git, Coucou) normalize external events into domain objects.
- Render tool data as untrusted text: sanitize rich output and never execute it as HTML/JS by default.
- Keep secret storage, permission policy, filesystem access, and command execution outside the presentational design-system layer.
- Use adapters/capability detection: if a provider cannot pause, estimate cost, or report context, the UI shows `Unavailable`, not placeholders disguised as real data.

## 9. Acceptance criteria / QA matrix

### Visual system
- [ ] Dark and light theme tokens centralized; no feature-level hardcoded palette.
- [ ] Default, hover, focus, disabled, selected, loading, and error stories for each applicable component.
- [ ] Responsive screenshots for 800, 1100, 1440, and 1920 px widths.
- [ ] Tested contrast in both themes and in selected/error states.
- [ ] Reduced motion and font scaling validated.

### Functionality
- [ ] Mode switching retains project context and selected artifacts.
- [ ] Command palette navigates all modes and differentiates side-effectful commands.
- [ ] Agent status reflects authoritative events and supports `unknown`/stale.
- [ ] Approvals explain scope and cannot be bypassed by UI state confusion.
- [ ] Proposed changes visibly differ from saved changes.
- [ ] Diff/commit flow guards against accidental loss.
- [ ] No fake test results, progress percentages, or cost numbers.
- [ ] Keyboard-only completion of project switch, chat send, approval, navigation, diff review, and log inspection.

### Testing
- [ ] Unit tests for token helpers, state mapping, and core component logic.
- [ ] Component tests for keyboard, disabled/loading, and approval actions.
- [ ] Accessibility checks using axe-core plus manual keyboard/screen-reader smoke tests.
- [ ] Playwright end-to-end tests for flows A–D with mocked adapters.
- [ ] Visual regression snapshots for shell, task cards, dialogs, diffs, and theme variants.
- [ ] Adapter contract tests for event/state normalization.

## 10. Build priorities

**Phase 0 — Audit:** Inspect repository, technologies, existing files, design assets, dependency graph, and actual integration capabilities. Preserve working code. Identify where UI currently exists.

**Phase 1 — Foundations:** Implement semantic tokens, themes, typography, spacing, accessible primitives, Storybook, and AppShell with persistent mode navigation.

**Phase 2 — Chat + Agents:** Implement conversation, composer, task cards, agent run inspector, approval dialog, and authoritative task status integration.

**Phase 3 — Code + Git:** Integrate editor, file browser, diff review, status display, terminal panel, source-control interface.

**Phase 4 — Knowledge + Design:** Implement note/block references, backlinks, component catalog, token explorer, preview and proposed AI changes.

**Phase 5 — Monitor + Routing:** Implement event stream, filterable logs, route status, reliable provider usage reporting, and alerts.

**Phase 6 — Hardening:** Accessibility, visual regression, performance testing, integration contracts, rollback safety, documentation.

### Definition of done per task

`Read relevant docs/files → Plan → Add to task tracking → Implement → Fix issues → Write tests → Run tests → Review → Report`.

For complex tasks, orchestrate planning, implementation, testing, and independent review as separate responsibilities **where supported by the available agent tooling**. Never claim unavailable agents or tests actually ran.

---

## 11. Claude Code execution prompt

> You are implementing **Coding Agent Design System v0.2** using this document as the authoritative UX and component specification.
>
> 1. **Inspect first.** Read project instructions (`CLAUDE.md`, README, architecture docs), existing UI/design-system components, repository structure, scripts, tests, and integration adapters. Do not assume the suggested stack is installed or correct.
> 2. **Report a concise initial summary**, then a phased implementation plan with concrete files and acceptance criteria. Ask only where a missing decision is genuinely blocking; otherwise choose reversible defaults and state assumptions.
> 3. **Maintain a task table** in the repository's existing tracker or a minimal Markdown file if no tracker exists. Do not overwrite established planning conventions.
> 4. **Implement incrementally**, prioritizing tokens, shell, and shared primitives before feature panels. Integrate existing functionality rather than duplicating systems.
> 5. **Use a verification pipeline**: inspect → edit → fix → test → review → report; use specialist subagents when actually available and proportionate.
> 6. **Treat external capabilities as unknown until verified.** Build adapters and capability-driven UI for Hermes Agent, OpenCode, Open Crew, Logseq, Coucou, 9Router, and Git. Do not invent endpoints or undocumented integration behavior.
> 7. **Run the project's actual checks** (lint/typecheck/unit/component/e2e/accessibility as applicable). Report exact commands and results. Never represent mocked tests as live integration verification.
> 8. **Respect security boundaries.** Do not expose credentials, auto-approve destructive commands, or silently change user files outside authorized scope.
> 9. **End each phase** with a summary of completed work, changed files, test results, open risks, and the next small milestone.
>
> **Begin by auditing the current repository and producing the Phase 0 findings and Phase 1 task breakdown. Do not rewrite the application blindly.**

## 12. Decisions still open

These should be captured as explicit decisions rather than guessed:

- Final product name and wordmark (current mark is a conceptual identity).
- Desktop runtime (Tauri, Electron, native, or browser shell).
- Persistence and synchronization strategy for workspace layouts and knowledge.
- Role/permission model for single-user vs team workflows.
- Actual API, licensing, and compatibility constraints of each listed integration.
- Repository-specific feature priorities and existing design-system conventions.

**End of v0.2.**
