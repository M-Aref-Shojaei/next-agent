---
title: Risks and Open Questions
tags: [plan, risks]
status: living
---

# Risks and Open Questions

## Risks
| ID | Risk | Impact | Mitigation |
|----|------|--------|------------|
| R1 | App-level sandbox only (M1) — a permitted command like `pnpm test` can run arbitrary scripts that escape the workspace | High | Be explicit in UI that `auto` is policy-level; OS sandboxing in M2; worktree limits blast radius |
| R2 | Command classification by parsing is bypassable (aliases, `sh -c`, `eval`, encoded payloads, scripts) | High | Default-deny unknowns in `auto` (ask); treat interpreters with `-c`/`-e` as `unknown`; fuzz corpus |
| R3 | Upstream licences incompatible with intended distribution | High | Licence review in M0 before any fork ([[Upstream Evaluation]]) |
| R4 | Upstream churn (opencode, 9router, openclaw move fast) | Medium | Adapter boundaries; pin versions; no deep forks in M1 |
| R5 | Sidecar runtimes (Node/Bun/Python) bloat install & complicate signing | Medium | [[ADR-0007 Minimize Sidecars in M1]] |
| R6 | Windows path semantics (junctions, 8.3 names, UNC, case) | Medium | Dedicated Windows test corpus; use `dunce` for canonicalization |
| R7 | Overlay window behaviour differs per OS (notch API is macOS-only) | Low | Platform-specific fallback in M3 |
| R8 | "Immutable" audit log on a user-owned machine can't be truly immutable | Medium | Append-only + hash chain + optional export/anchor; document the limitation |

## Open questions
- **Q1** ~~Licence for Next Agent itself?~~ **Resolved:** AGPL-3.0-or-later ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]).
- **Q2** ~~Bundle the router or keep it user-run?~~ **Resolved:** neither. The chosen 9Router features are ported into the Rust core ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]).
- **Q3** Is Bun acceptable for sidecars, or Node only? Assumption: Node 22+ LTS, Bun per-service if upstream requires.
- **Q4** Auto mode on non-git workspaces: refuse, or `git init` a shadow repo? Assumption: refuse (fall back to manual).
- **Q5** Default merge strategy for worktree branches: squash vs merge commit?
- **Q6** `Shift+Tab` conflicts with reverse-tab focus navigation (accessibility). Assumption: global only when chat input is focused or via configurable shortcut.
- **Q7** Does "Always for this workspace" persist across restarts and app updates? Assumption: yes, stored in `permission_rules`, visible/revocable in Settings.
- **Q8** Where do the app's own vault records go — user-selected vault, or `.next-agent/` inside the repo? Assumption: user vault, with per-workspace subfolder.
