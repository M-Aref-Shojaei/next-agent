---
title: ADR-0011 Vendor Whole Upstream Codebases on Request
tags: [adr]
status: accepted
---

# ADR-0011 Vendor Whole Upstream Codebases on Request

- **Status:** accepted (owner rule) · **Date:** 2026-11-14 · **Deciders:** the owner
- Amends [[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]: its rule 2, "extract, don't embed", no longer applies to an upstream the owner asks to vendor whole. Coucou is the first.

## Context
- The overlay was a partial port of Coucou (MIT code): the island only, re-implemented in Svelte. The owner wants all of Coucou's functionality, exactly as upstream, and will ask for improvements themselves.
- Coucou already ships a Tauri 2 app (`windows/`: Rust, framework-free TypeScript, a hook relay), the same stack as ours. Its Mac app is Swift.
- Coucou's name, the Mochi character, icons, sounds and media are not under its MIT licence (`LICENSE-ASSETS.md`).

## Decision
1. **Vendor whole, on request.** When the owner asks, an upstream's codebase is copied into the repo file for file, in its own folder, with its structure, names, constants and logic. Coucou's Tauri app lives in `apps/desktop/coucou/` (mirroring upstream `windows/`): `src-tauri/` and `hook/` are workspace crates, `src/` and `tests/` are its TypeScript.
2. **Translate only where the stack differs.**
   - Coucou's Tauri app ships for Windows and Linux. Its macOS layer (`platform/macos.rs`, `flow/os_macos.rs`, `hook/src/mac.rs`) is translated from the Swift app.
   - The AppKit and libc calls live in `crates/macos-panel`, still the only crate allowed `unsafe` on macOS.
3. **Convert config only where one app requires it.**
   - One builder: Coucou's `run()` is split into `builder` / `invoke_handler` / `setup` / `on_run_event`.
   - One `tauri.conf.json`, with `coucou-*` window labels and SvelteKit routes as mount points (`src/routes/coucou/*`).
   - A strict CSP that adds only upstream's `media-src 'self' blob:`.
   - A capability with `core:default`.
4. **Keep upstream's form.**
   - No reformatting: `apps/desktop/coucou/rustfmt.toml` disables rustfmt there.
   - The default Clippy lints that flag upstream's style are allowed by name in its `Cargo.toml`. Next Agent's pedantic table does not apply to it.
   - Upstream's `unsafe` stays, in its Windows and Linux files (allowed per file) and in one item.
   - Heuristic repo checks skip the folder, because the compiler checks it.
   - Every change to an upstream file is marked `Next Agent:` in a comment, so a re-sync is a plain diff.
5. **Assets stay out.**
   - No icon, sound, screenshot or media is copied (`scripts/check-no-coucou-assets.mjs`).
   - The code that draws the Mochi character draws a neutral gradient orb instead (`src/mochi/orb.ts`).
   - Every spot that uses Coucou's art, sound or name is listed in [[Coucou art to replace]].
6. **Connect through upstream's own seams.**
   - Next Agent's task events enter Coucou's hook input path (`pipe.rs` `connect_in_process`), translated to hook events by `apps/desktop/src-tauri/src/coucou.rs`.
   - Decisions return through `approval_respond`, so the permission engine still decides.
   - The Claude Code hook path stays intact, and is off until the user installs it from Coucou's settings.
7. **Pin and credit.**
   - `upstream.lock.json` records the vendored commit.
   - Every file carries `Ported from Coucou (MIT, © 2026 Louis Raillé): <path> @ <commit>`, except JSON, which has no comment syntax. Those files are listed in [[Coucou]].

## Options considered
| | Partial port (ADR-0009) | Vendor whole (chosen) | Upstream as a separate app |
|---|---|---|---|
| Fidelity to upstream | low: re-implemented | exact | exact |
| One app, one installer | yes | yes | no (two apps, two processes) |
| Re-sync cost | high: hand diff | low: `diff` against the pin | none |
| Code we carry | small | large (~50k Rust, ~27k TS lines) | none |
| Licence fit | fine | fine: MIT in AGPL; assets excluded | name and art not allowed to ship |

## Consequences
- **Easier:**
  - All of Coucou works inside Next Agent: island, chat, integrations, dictation, Notetaker, voice, phone link, recap, update notices.
  - Re-syncing is mechanical.
- **Harder:**
  - Much more code in the tree. Upstream's own dependencies come with it, including sherpa-onnx and onnxruntime (static), cpal, rustls, mdns-sd and lopdf.
  - Some features are Windows-only upstream (the Outlook mail, the WhatsApp installer) and stay so.
- **To revisit:**
  - The owner's artwork and name.
  - Bundling the hook relay as a resource.
  - Mac features of the Swift app that the Tauri app never had (see [[Coucou]]).
