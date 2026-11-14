---
title: Design Studio Sources
tags: [upstream, design]
status: researched
---

# Design Studio Sources

The owner used Open Design and found **serious flaws in its UI, UX and functionality**. So we design the studio's UX ourselves and port only proven *mechanics* from each tool.

| Tool | Licence | Stack | Mechanic we take | Mechanic we leave |
|---|---|---|---|---|
| [Open Design](https://github.com/nexu-io/open-design) | Apache-2.0 | Next.js, Node daemon, Electron | `DESIGN.md` design systems; `<artifact>` HTML in a sandboxed iframe; direction picker; self-critique; SSRF guard | Its UI and UX; video, PPTX, plugin marketplace and cloud |
| [Onlook](https://github.com/onlook-dev/onlook) | Apache-2.0 | Next.js, Supabase, CodeSandbox | Runs the real app in a preview and **maps each element to its place in the source**; applies a visual edit to the preview first, then to the code; right-click opens the source; checkpoints | Supabase, CodeSandbox and hosting. The open-source editor is the predecessor of a hosted product, so check its activity before porting |
| [OpenUI](https://github.com/wandb/openui) | Apache-2.0 | Python backend, npm frontend | Describe a UI → live HTML; **convert HTML to React, Svelte or Web Components** | The Python backend and LiteLLM |
| [open-pencil](https://github.com/open-pencil/open-pencil) | MIT (*verify*) | **Tauri** + Vue (same shell as ours) | Later: read and write Figma `.fig` files; its headless design SDK | Not production-ready yet. Revisit when it is stable |

## Why this beats copying one tool
- Open Design is good at **generating from a design system**, but its editing loop is weak.
- Onlook is good at **editing the real product**, so the design ends up in the code instead of a throwaway mockup. Next Agent already has the codebase, worktrees, a coding agent and diff review, so this fits.
- OpenUI fills the gap between a prototype and framework code.

## How it fits our architecture
- **Model:** generation uses the LLM gateway's `design` model role.
- **Writes:** every write goes through the broker and the permission engine, inside a task worktree. You review the diff and merge.
- **Previews:** run in a sandbox, with network access blocked by default using Open Design's SSRF rules.
- **Credit:** each ported file has a header, and each source is listed in `NOTICE.md`. Apache-2.0 §4 requires us to mark files we changed and keep the upstream NOTICE text.

Sources:
- [Onlook repo](https://github.com/onlook-dev/onlook)
- [OpenUI repo](https://github.com/wandb/openui)
- [open-pencil repo](https://github.com/open-pencil/open-pencil)
- [Open Design repo](https://github.com/nexu-io/open-design)
- [Best AI design tools roundup](https://open-design.ai/blog/best-ai-design-tools/)
