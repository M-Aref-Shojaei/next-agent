# Notices and credits

Next Agent is licensed under the GNU Affero General Public License v3.0 or later (see `LICENSE`).

It re-implements or ports selected features from the open-source projects below ([[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]). It bundles none of their runtimes. Every ported file starts with a header naming the project and the source file it came from. Code copied from an MIT-licensed project keeps that project's copyright notice and the MIT licence text below. The commit each port is based on is recorded in `upstream.lock.json`.

| Project | Repository | Licence | Copyright | What we take |
|---|---|---|---|---|
| 9Router | https://github.com/decolua/9router | MIT | © 2024-2026 decolua and contributors | Caveman / Ponytail prompt injection, RTK tool-output compression, model combos and fallback, provider translation (OpenAI ↔ Anthropic ↔ Gemini), usage and cost reports |
| opencode | https://github.com/anomalyco/opencode | MIT | © 2025 opencode | Context compaction (`crates/core/src/compaction.rs`), tiered edit matching (`edit_match.rs`), shadow-git snapshots/undo (`worktree.rs` `ShadowRepo`, `snapshot.rs`), Markdown agent profiles (`profiles.rs`) and subagents (`subagent.rs`), permission-rule refinements |
| Hermes Agent | https://github.com/NousResearch/hermes-agent | MIT | © 2025 Nous Research | Skills (SKILL.md, progressive disclosure, curator), curated memory, session search, background review, cron scheduling, prompt-cache rules (`builtin.rs` `validate_transcript`), delegate-tool design merged into `subagent.rs` |
| OmniRoute | https://github.com/diegosouzapw/OmniRoute | MIT | © 2026 diegosouzapw | Exact-match response cache, per-provider circuit breaker, prompt-cache affinity, combo orderings (weighted, cost-optimized, last-known-good, context-optimized), the budget-cap idea |
| OpenRouter (docs only) | https://openrouter.ai/docs | n/a: closed-source service; **no code taken** | — | Ideas re-implemented from public docs: models catalog import (fetched by the user on demand, never bundled), cache-aware cost, unified reasoning setting, context-overflow model switch |
| OpenClaw | https://github.com/openclaw/openclaw | MIT | © 2026 OpenClaw Foundation | Channel adapter design, DM pairing and allow-lists, session routing, group gating, outbound chunking, scheduler |
| Coucou | https://github.com/Louis-CFM/coucou | MIT (code only) | © 2026 Louis Raillé | Overlay window technique: notch/corner placement, click-through, approval queue. **Not** the Coucou name, the Mochi character, icons, sounds or other assets, which are excluded from its MIT licence. |
| Excalidraw | https://github.com/excalidraw/excalidraw | MIT | © 2020 Excalidraw | Embedded as a library for the Design board (npm `@excalidraw/excalidraw` 0.18.1, run through a React island; fonts are copied locally at build time). Holder taken from the LICENSE file in the repository. |
| @excalidraw/mermaid-to-excalidraw | https://github.com/excalidraw/mermaid-to-excalidraw | MIT | © 2023 Excalidraw | Mermaid text to Design board elements (npm 3.6.0; licence checked in the package) |
| Open Design | https://github.com/nexu-io/open-design | Apache-2.0 (some bundled templates MIT, see its README) | © Open Design contributors (*verify holder in its NOTICE*) | DESIGN.md design-system format, AI design generation flow (sandboxed HTML artifacts), direction picker, self-critique, design templates (each licence checked before use). Files we derive from it state that we changed them, as Apache-2.0 §4 requires. |
| Onlook | https://github.com/onlook-dev/onlook | Apache-2.0 | © Onlook contributors (*verify holder*) | Mapping elements to their source and writing visual edits back to code, for the Design Studio |
| OpenUI | https://github.com/wandb/openui | Apache-2.0 | © Weights & Biases (*verify holder*) | Converting HTML to React, Svelte or Web Components |
| Logseq | https://github.com/logseq/logseq | AGPL-3.0 | Logseq contributors | Markdown graph format (blocks, properties, refs, journals), triple-lowbar file names, outliner behaviour, backlinks, queries, graph view. Studied tag 0.10.9 @ c67b8b5 (`crates/graph`, `packages/vault`) |

## MIT licence text (applies to code ported from the MIT projects above)

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
