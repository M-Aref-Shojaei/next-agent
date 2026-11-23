---
title: Design System v0.2 Implementation
 tags: [plan, design, ui]
status: active
---

# Design System v0.2 Implementation

Source specification: [[Coding Agent Design System v0.2]]. Existing app stack remains **Tauri 2 + SvelteKit**. Suggested React/Tailwind/Storybook stack is not installed and will not be introduced without a separate decision.

## Phase 0 — Audit findings

- Existing design tokens: `apps/desktop/src/lib/ds/tokens.css`.
- Existing shared components: `apps/desktop/src/lib/ds/`.
- Existing living style guide: `/settings/design-system`.
- Existing shell: `apps/desktop/src/routes/(app)/+layout.svelte` with sidebar, main content, status bar, and narrow drawer behavior.
- Existing automated guard: `scripts/check-design-tokens.mjs`.
- Existing UI test direction: Playwright config and workflow; results tracked in [[UiTestResults]].
- Existing implementation differs from v0.2: shell has no activity rail, inspector/bottom-panel primitives are incomplete, and several modes are represented by routes rather than a persistent rail.

## Phase 1 — Foundations

| Item | Status | Acceptance |
|---|---|---|
| Preserve existing token/component system | done | No stack migration; current checks remain valid |
| Add v0.2 semantic token aliases | in progress | Dark/light themes expose canvas, surface, elevated, hover, status, focus names |
| Add shell geometry tokens | in progress | Shell dimensions come from semantic tokens |
| Implement persistent activity rail | in progress | Keyboard-accessible Chat/Code/Agents/Knowledge/Design/Source Control/Monitor navigation |
| Implement command palette | pending | Cmd/Ctrl+K; risky commands show side-effect preview |
| Add responsive inspector/bottom panel primitives | pending | Collapsible below 1440px; no unusable editor widths |
| Add token/component preview matrix | ready for approval | Light/dark, density, and viewport controls plus status-state cards visible in style guide |

## Review gates

Every UI slice must:

1. Use tokens and shared components.
2. Add or update a focused test.
3. Pass `node scripts/test.mjs`, `pnpm build`, `svelte-check`, and design-token checks.
4. Add a row to [[UiTestResults]] before owner approval.
5. Avoid claiming unsupported provider, progress, cost, or test state.

## Decisions not guessed

- Keep SvelteKit rather than migrate to React/Tailwind/Storybook.
- Keep current token names as compatibility API; add semantic v0.2 names incrementally.
- Use existing Tauri commands/adapters for real state; no invented integration endpoints.
