---
title: Open Design
tags: [upstream]
status: researched
---

# Open Design — what we port

- **Repo:** https://github.com/nexu-io/open-design
- **Licence:** **Apache-2.0**. It is compatible with our AGPL-3.0: Apache-2.0 code can be included, but we must keep its `NOTICE`/attribution and mark any files we change.
- **Bundled parts with other licences:** these keep their own licences.
  - `design-templates/guizang-ppt/` (MIT, @op7418)
  - `design-templates/html-ppt/` (MIT, @lewislulu)
  - `skills/web-clone/` (MIT, @Jane-xiaoer)
- **Stack:** Next.js 16 and React. A Node 24 Express daemon with SQLite. Electron shell.
- **Our approach:** port the ideas and file formats into Rust and Svelte. Bundle none of their Node daemon or Electron shell ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]).
- The owner has used it and wants this workflow inside Next Agent.

## What it does
An agent designs **from the project's design system**. It writes HTML prototypes, dashboards and decks, which render live in a sandboxed preview. The user iterates by chatting with the agent.

## Parts we port

### 1. `DESIGN.md` design systems (`design-systems/<brand>/`)
- A Markdown "brand contract": colours, type, spacing, components, voice and so on.
- Newer packages add `manifest.json`, `tokens.css`, components, assets and provenance.
- The original schema had nine sections. *Verify* their names in `design-systems/README.md` before porting.
- **For us:** each workspace gets `DESIGN.md` (plus optional `tokens.css`) in the repo or vault. The studio can **generate it from the project's existing CSS tokens**.
  - Next Agent's own design system comes from `apps/desktop/src/app.css`.
- 151 ready-made brand packages exist upstream. Check each package's licence before bundling any of them.

### 2. Generation flow
- **Agents with file tools:** the agent writes real files and the preview renders them.
- **Models without file tools:** the model returns one complete `<artifact>` block, which is parsed and rendered in a **sandboxed `srcdoc` iframe**.
- **For us:**
  - Agents with file tools go through our broker. Writes stay inside the design folder and get permission-engine checks.
  - The `<artifact>` path suits plain model calls.

### 3. Assigned models
- Upstream spawns CLI agents or calls BYOK proxies (Anthropic, OpenAI, Google, Ollama, OpenAI-compatible).
- **For us:** the studio uses a **model role** (`design`) from the LLM gateway. The owner assigns a model or combo to it in Settings.

### 4. Iteration UX
- Chat in the same project.
- A question form when the request is ambiguous.
- A **five-direction picker**: several design directions to choose from.
- Todo progress.
- A **five-dimension self-critique** before output, plus an artifact lint API.
- Comment-mode "surgical edits" are only partly shipped upstream.

### 5. Templates, skills and prompts
Each one needs a licence check before we take it.
- `design-templates/`: `web-prototype`, `dashboard`, `critique`, `tweaks` and others.
- `skills/`: 100+ `SKILL.md` files, the same format as [[Hermes Agent]] skills.
- `prompt-templates/`: 93 prompts.

### 6. Outputs
- **Ported first:** HTML prototypes in web, mobile and desktop frames.
- **Optional:** decks as HTML or PDF via print.
- **Skipped:** video (HyperFrames: headless Chrome and FFmpeg), PPTX, image generation.
- **Handoff:** the prototype becomes a task for our coding agent, which implements it in the real codebase.

## Security notes to copy
- **SSRF guard:** block private, link-local, CGNAT and cloud-metadata addresses by default.
- **Sandbox:** previews run in a sandboxed iframe, isolated from the app.

## Skip
- Electron shell, Node daemon, CLI-agent spawning (we have our own agents), MCP server, video, the plugin marketplace (277 plugins), OpenDesign Cloud.

## Read before porting
- `docs/architecture.md`
- `docs/skills-protocol.md`
- `design-systems/README.md`
- `plugins/spec/SPEC.md`
- `AGENTS.md`
