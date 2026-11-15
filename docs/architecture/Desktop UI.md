---
title: Desktop UI
tags: [architecture, ui]
status: draft
---

# Desktop UI

SvelteKit (SPA via `adapter-static`) + Vite inside Tauri. See [[ADR-0002 SvelteKit Frontend]].
Every screen is built from the design system in `apps/desktop/src/lib/ds` ([[Design System]], [[ADR-0012 One Design System]]).

## Layout
```
┌ Sidebar ─────┬ Timeline ───────────────────────────┬ Panel ───────────────┐
│ Workspaces   │ user / agent messages               │ Diff | Files | Vault  │
│ Tasks        │ tool cards (collapsible output)     │                       │
│ Kept branches│ approval cards [Allow][Deny][Always]│ side-by-side diff     │
│              │ ─────────────────────────────────── │ [Merge][Discard][Keep]│
│              │ chat input                          │                       │
├──────────────┴─────────────────────────────────────┴───────────────────────┤
│ [AUTO] mode badge · router ● ok · branch next-agent/auto-3f2a · 1 pending   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Mode badge
- `Plan` (blue), `Manual` (neutral), `Auto` (amber), `Bypass` (red, persistent banner).
- Click → menu; `Shift+Tab` cycles Plan → Manual → Auto. Bypass only through confirmation dialog (type the workspace name).

## Components (packages/ui)
`ModeBadge`, `Timeline`, `MessageItem`, `ToolCard`, `ApprovalCard`, `DiffViewer` (CodeMirror 6 merge view), `RouterStatus`, `SettingsForm`, `RuleEditor`, `AuditTable`.

## State
Svelte 5 runes stores per task, fed by Tauri `Channel` events; reconcile by `(task_id, seq)`.

## Settings screens
Router · Vault · Permissions (rules list, add/remove, scope) · Audit log (read-only, verify chain) · Workspaces (orphaned worktrees).
