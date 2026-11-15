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
| OpenClaw | https://github.com/openclaw/openclaw | MIT | © 2026 OpenClaw Foundation | Channel adapter design, DM pairing and allow-lists, session routing, group gating, outbound chunking, streaming previews, untrusted-content wrapping (`crates/channels`, studied at commit `1ac07b3`; files listed in `upstream.lock.json`). |
| Coucou | https://github.com/Louis-CFM/coucou | MIT (code only) | © 2026 Louis Raillé; phone link and `android/` © 2026 Rhytam Biswas (MIT) | The whole Tauri app (Rust, TypeScript, hook relay, tests), vendored at commit `0d7fe58` in `apps/desktop/coucou/` ([[ADR-0011 Vendor Whole Upstream Codebases on Request]]); its macOS layer translated from the Swift app (`platform/macos.rs`, `flow/os_macos.rs`, `hook/src/mac.rs`, `crates/macos-panel`). Every file has a header naming its upstream path, except JSON files (listed in `docs/upstream/Coucou.md`). **Not** the Coucou name in new work, the Mochi character, the app and menu-bar icons, the sounds or other media: Coucou's `LICENSE-ASSETS.md` keeps them out of its MIT licence. None of them is in this repository; a neutral gradient orb is drawn where Coucou draws Mochi; `docs/upstream/Coucou art to replace.md` lists every spot; `scripts/check-no-coucou-assets.mjs` guards it. |
| sherpa-onnx (with onnxruntime) | https://github.com/k2-fsa/sherpa-onnx | Apache-2.0 (onnxruntime: MIT) | © Xiaomi Corporation and contributors; onnxruntime © Microsoft | Linked statically by the vendored Coucou crate for on-device speech to text (Flow dictation, Notetaker). The speech models (NVIDIA Parakeet) are downloaded only on the user's click and are not in this repository. **Verify** the model licence before shipping. |
| Excalidraw | https://github.com/excalidraw/excalidraw | MIT | © 2020 Excalidraw | Embedded as a library for the Design board (npm `@excalidraw/excalidraw` 0.18.1, run through a React island; fonts are copied locally at build time). Holder taken from the LICENSE file in the repository. |
| @excalidraw/mermaid-to-excalidraw | https://github.com/excalidraw/mermaid-to-excalidraw | MIT | © 2023 Excalidraw | Mermaid text to Design board elements (npm 3.6.0; licence checked in the package) |
| React and React DOM | https://github.com/facebook/react | MIT | © Meta Platforms, Inc. and affiliates | Run Excalidraw's React island in the Design board (npm `react` and `react-dom` 19.3.1, exact; lazy-loaded on the board route only) |
| Excalidraw fonts | bundled inside `@excalidraw/excalidraw`, copied to `/excalidraw-assets/fonts/` at build time | Per font, read from each font file's own metadata: Assistant, Nunito, Lilita One, Virgil, Cascadia Code (Microsoft): SIL OFL 1.1. Comic Shanns: MIT. Excalifont (© 2024 Excalidraw) and Xiaolai (© 2020 LXGW): no licence text in the file, upstream describes both as open fonts, **verify** (expected OFL 1.1). Liberation Sans: the file carries a 2007 Ascender copyright and no licence text; Liberation 1.x is GPL-2.0 with the font exception and 2.x is OFL 1.1, **verify which** | See the font's own copyright line (Assistant Project Authors, Nunito Project Authors, Juan Montoreano, Microsoft, Shannon Miwa and others) | Fonts the canvas draws with, served from the app itself so the CSP needs no CDN. No font is modified. |
| DOMPurify | https://github.com/cure53/DOMPurify | MPL-2.0 OR Apache-2.0 (we use it under Apache-2.0) | © Cure53 and other contributors | HTML sanitising inside Mermaid (transitive dependency of `@excalidraw/mermaid-to-excalidraw`, bundled in the lazy Mermaid chunk) |
| Chevrotain | https://github.com/Chevrotain/chevrotain | Apache-2.0 | © Shahar Soel and contributors | Parser toolkit used by Mermaid (transitive, lazy Mermaid chunk). Unmodified; licence text ships in the package. |
| Khroma | https://github.com/fabiospampinato/khroma | MIT | © 2019-present Fabio Spampinato, Andrew Maney | Colour utilities used by Mermaid (transitive, lazy Mermaid chunk). Licence read from the package's `license` file. |
| Open Design | https://github.com/nexu-io/open-design | Apache-2.0 (some bundled templates MIT, see its README; none taken) | Copyright 2026 Open Design contributors (from its LICENSE; it ships no NOTICE file) | Studied `main` @ `59e4f2a`. `crates/design`: `DESIGN.md` section layout (`system.rs`), `<artifact>` parsing (`artifact.rs`), direction postures, output contract and five-dimension self-critique (`prompts.rs`, `critique.rs`), artifact lint rules (`lint.rs`), preview sandbox and CSP (`sandbox.rs`), SSRF address ranges (`ssrf.rs`). Each file says it is changed, as Apache-2.0 §4 requires. No templates, design systems or UI taken. |
| Onlook | https://github.com/onlook-dev/onlook | Apache-2.0 | Copyright 2024 On Off, Inc. (from its LICENSE.md; it ships no NOTICE file) | Studied `main` @ `423e2e9`. Element → source mapping (`crates/design/src/source_map.rs`, the in-page `bridge.js`, `packages/design-instrument`) and visual edits written back to code (`codemod.rs`). Each file says it is changed. |
| OpenUI | https://github.com/wandb/openui | Apache-2.0 | Copyright 2024 Weights and Biases, Inc. (from its LICENSE; it ships no NOTICE file) | Studied `main` @ `42d7ab4`. HTML → React / Svelte / Vue / Web Component conversion prompt and framework list (`crates/design/src/convert.rs`, `prompts.rs`). Each file says it is changed. |
| Logseq | https://github.com/logseq/logseq | AGPL-3.0 | Logseq contributors | Markdown graph format (blocks, properties, refs, journals), triple-lowbar file names, outliner behaviour, backlinks, queries, graph view. Studied tag 0.10.9 @ c67b8b5 (`crates/graph`, `packages/vault`) |
| d3-force | https://github.com/d3/d3-force | ISC | © 2010-2026 Mike Bostock (with d3-dispatch, d3-quadtree, d3-timer, same licence) | Force layout of the Notes graph view (npm 3.0.1, exact). Canvas drawing is our own. Unmodified; licence text ships in the package. |
| Mermaid | https://github.com/mermaid-js/mermaid | MIT | © 2014-2022 Knut Sveidqvist | Rendering ```` ```mermaid ```` blocks in notes (npm 11.17.2, exact). Lazy-loaded chunk, `securityLevel: "strict"`, bundled with the app so the CSP needs no remote script. Unmodified. |

## Apache-2.0 projects

Open Design, Onlook and OpenUI are licensed under the Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0). None of them ships a `NOTICE` file, so there is no upstream NOTICE text to carry over (Apache-2.0 §4(d)). Files derived from them start with a header naming the upstream file and commit and stating that we modified it (§4(b)).

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
