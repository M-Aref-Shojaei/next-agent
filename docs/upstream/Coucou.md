---
title: Coucou
tags: [upstream]
status: vendored
---

# Coucou: vendored whole

- **Repo:** https://github.com/Louis-CFM/coucou
- **Licence:** MIT for the code, © 2026 Louis Raillé. The phone link and `android/` are © Rhytam Biswas, also MIT.
- **Not covered by the licence:** the name, Mochi, icons, sounds and media (`LICENSE-ASSETS.md`). None are copied. See [[Coucou art to replace]].
- **Pin:** `main @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8`, the latest `main` on 2026-11-14 (`upstream.lock.json`).
- **Decision:** [[ADR-0011 Vendor Whole Upstream Codebases on Request]]. It replaces the partial island port that ADR-0009 allowed.

## Layout

| Upstream | Ours |
|---|---|
| `windows/src-tauri/` (Rust app) | `apps/desktop/coucou/src-tauri/` (workspace crate `coucou`, lib `coucou_lib`) |
| `windows/hook/` (relay) | `apps/desktop/coucou/hook/` (crate `coucou-hook`) |
| `windows/src/` (TypeScript UI) | `apps/desktop/coucou/src/` (alias `$coucou`) |
| `windows/tests/` | `apps/desktop/coucou/tests/`, run by `scripts/test.mjs` with its `setup.mjs` |
| `windows/{dev,scripts,nsis}/`, `README.md`, `package.json`, `tsconfig.json` | same paths. They are copied as is: the dev previews, the generators and the NSIS hook are not wired into Next Agent's build, and `tsconfig.json` runs in `pnpm check`. |
| `windows/{index,settings,mochi,pill}.html` | the SvelteKit routes `/coucou/{island,settings,mochi,pill}`, which mount the same element and import the same entry module |
| `windows/vite.config.ts` | not copied: SvelteKit builds the pages. Its shared-sounds plugin served assets we do not ship. |
| `windows/src-tauri/tauri*.conf.json`, `capabilities/default.json`, `build.rs`, `src/main.rs` | merged into Next Agent's one app (see below) |
| `android/relay/test-vectors.json`, `android/LICENSE` | `apps/desktop/coucou/android/relay/`, read by the phone-link crypto tests |
| `windows/src-tauri/icons/`, `windows/screenshots/`, `windows/Cargo.lock`, `package-lock.json`, workspace `Cargo.toml` | not copied: assets, and lock files that the workspace replaces |

Every file has a `Ported from Coucou (MIT, © 2026 Louis Raillé): <path> @ <commit>` header, except the files with no comment syntax:
- `package.json`
- `tsconfig.json`
- `src/i18n/strings.json`
- `src/i18n/extra.json`
- `src/whats-new/whats-new.json`
- `src-tauri/src/calcom/defaults.json`
- `android/relay/test-vectors.json`
- `android/LICENSE`

This page and NOTICE.md credit them instead. Every change to an upstream file carries a `Next Agent:` comment.

## Inventory

### Tauri app features (all copied)

| Feature | Files |
|---|---|
| Island window | `island.rs`, `platform/*`, `webview_memory.rs`, `island/{island,fsm}.ts`, `core/{layout,closed,anim}.ts` |
| Claude Code and agent hooks, approvals, questions | `hook/`, `pipe.rs`, `hooks.rs`, `agents.rs`, `agent_hooks.rs`, `config_file.rs`, `agy_rules.rs`, `agy_approvals.rs`, `island/{hooks,agents}.ts` |
| Open terminal, launch a session, drop files | `session_window.rs`, `herdr.rs`, `launch.rs`, `files.rs`, `webview_drop.rs` |
| Chat | `chat.rs`, `claude.rs`, `openai_compat.rs`, `local_chat.rs`, `cli_relay.rs`, `claude_code.rs`, `net.rs` |
| Plan usage | `codex_plan.rs`, `hook/statusline.rs`, `core/plan.ts`, `views/usage.ts` |
| Integrations | `integrations.rs` (Stripe, Vercel, n8n, Resend, Notion), `github.rs`, `calcom.rs` |
| Mail (Outlook through COM, Windows only) | `mail/*` |
| WhatsApp (GOWA) | `whatsapp/*` |
| Spotify | `spotify.rs` |
| Flow dictation | `flow/*`, `speech/*` (Parakeet through sherpa-onnx), `portal.rs` |
| Notetaker | `notes/*` |
| "OK Coucou" voice | `voice/*` |
| Android phone link | `phone_link/*` |
| Desktop companion, wardrobe | `desktop.rs`, `src/mochi/*`, `src/desktop` |
| Weekly recap | `recap.rs`, `src/recap/*` |
| Settings and the hub window | `settings.rs`, `src/settings`, `src/hub` |
| Plumbing | `secrets.rs`, `tray.rs`, `shortcuts.rs`, `update.rs`, `sounds.rs`, `i18n.rs`, `log.rs`, `identity.rs`, `src/whats-new`, `src/upload` |

### Swift Mac app features and their status here

**Translated for macOS** (the Tauri app had no macOS build):

| Feature | Swift source | Ours |
|---|---|---|
| Notch `IslandPanel` | `IslandWindowController` | `platform/macos.rs` with `crates/macos-panel` (`adopt`, `place_frame`, `set_ignores_mouse`, `set_key`) |
| Cursor poll | `IslandWindowController` | same |
| Click-through | `IslandWindowController` | same |
| Focus the terminal | `TerminalTarget+AppKit` | `focus_session_window` (`open -a <bundle>`) |
| Launch a session | `LaunchScript`, `SessionLauncher` | `launch.rs` (a `.command` file opened in Terminal) |
| Folder picker | `NSOpenPanel` | AppleScript `choose folder` |
| User's name | `NSFullUserName` | `macos-panel::sys` |
| Keychain | | `keyring` with `apple-native` |
| Flow keys | `FlowShortcut` | polled key state, `flow/os_macos.rs` |
| Flow paste | `FlowDictation` | ⌘V through `CGEventPost` |
| App in front, running apps | `NSWorkspace` | `macos-panel::sys` |
| Voice: open an app, speak | | `/Applications` search, `say` |
| Relay socket | `HookServer` | `hook/src/mac.rs` |

**Already in the Tauri app, so copied:**
- hooks, approvals and questions;
- chat and the CLI relay;
- plan gauges;
- the pollers, GitHub and Cal.com;
- WhatsApp;
- Spotify (on a Mac: media keys not wired);
- Flow, the Notetaker and voice (with fewer engines);
- the desktop companion;
- the recap;
- updates and What's new;
- the Android link.

**Mac-only in Swift, not in the Tauri app, not translated:**

| Feature | Swift source |
|---|---|
| Corner island | `CornerIsland` |
| iPhone link | `PhoneLink/`: CloudKit, APNs, Live Activities |
| Apple Music | `MusicController` |
| Apple Mail | `MailController` |
| Claude Desktop reply watcher | AX |
| Screen and window capture | `ScreenCapture`, `QuickScreenshot`, `WindowContextCapture` |
| Pomodoro timer | Coucoudoro |
| Crypto and portfolio | |
| Machine vitals, work streak | IOKit |
| Apple speech recogniser, fn-key dictation | |
| Apple Intelligence summaries | FoundationModels |
| Contacts lookup | |
| Focus-mode quieting | |
| App Store review, demo engine | |

These are Swift-only code paths with no Tauri counterpart to copy. Each would be a new port.

## How it runs inside Next Agent

- **One builder.**
  - `apps/desktop/src-tauri/src/lib.rs` calls `coucou_lib::builder` (Coucou's plugins and state), `coucou_lib::setup` (its setup, plus the island window that its tauri.conf.json declared) and `coucou_lib::on_run_event`.
  - `coucou_lib::invoke_handler` routes Coucou's command names to Coucou's handler and everything else to Next Agent's.
- **Windows.**
  - Labels: `coucou-island`, `coucou-settings`, `coucou-mochi`, `coucou-pill`.
  - Capability `capabilities/coucou.json` (`core:default`).
  - CSP: Next Agent's, plus upstream's `media-src 'self' blob:`.
- **macOS.**
  - The island webview moves into the `IslandPanel` (level `mainMenu + 3`, `constrainFrameRect` passthrough), placed at the top centre of the chosen display by its frame in points.
  - The cursor poll flips `ignoresMouseEvents`.
- **Next Agent's tasks.**
  - `apps/desktop/src-tauri/src/coucou.rs` turns each `task-stream` event into the hook event Claude Code would send: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`/`PostToolUseFailure`, `Stop`, `StopFailure`, `Interrupt`.
  - Each event is tagged `coucou_agent: "next-agent"`, which gives Next Agent its own pill. It is written to `pipe.rs` `connect_in_process`, the same `handle` a relay connection goes through.
  - The island's Allow/Deny comes back on that stream and goes to `approval_respond`.
  - An approval answered in the main window closes the stream, and the card goes.
- **Claude Code.**
  - The relay and socket work as upstream: `~/Library/Application Support/dev.nextagent.desktop/coucou/coucou.sock`, a 0700 folder, and a uid check on both ends.
  - Hooks are written only from Coucou's settings (Agents → Claude Code), after its diff, backup and confirmation. Nothing is written until then.
- **Settings.** Next Agent's Settings → "Overlay / Coucou" opens Coucou's own settings window (`overlay_open`).

### Differences from upstream (each marked `Next Agent:` in the code)
- **Not at launch:**
  - Coucou's window does not open at launch.
  - Single-instance, autostart and global-shortcut plugins come with it.
  - `tauri-build`'s `cfg(dev)` is never set in the library.
- **The character:** drawn as the orb (`orb.ts`). `outfits.ts` keeps only the wardrobe icon.
- **Tests:**
  - Tests that read Coucou's Mac app or web site are skipped when those files are absent.
  - Two upstream tests shared a temp folder and raced on macOS; one now uses its own.
- **Lints and checks:**
  - Upstream's style lints are allowed by name, and its formatting is kept (`rustfmt.toml`).
  - The structure heuristic skips the folder.

## Upstream watch

- Settings → Upstreams lists what Coucou published since the pin: commits (title, date, scope, link), releases, and the areas the range changed.
- `crates/core/src/upstream/` reads the `watch` entry of `upstream.lock.json` (embedded at build time). It asks GitHub's public API once a day while the app runs, or on "Check now". It makes three unauthenticated requests: repository, compare from the pin, and releases.
- It sends no token and no user data. A rate limit (403/429 with GitHub's headers) is remembered, and no check runs until it ends. The last result is kept in `upstream-watch.json` in the app data folder.
- It never applies anything. Bringing a change in is a manual re-sync from the pin, then a re-pin of both `pin` and `watch.commit`.
- It is generic: another upstream joins by getting a `watch` entry.

## Not verified
- The Windows and Linux builds of the vendored crate: they cannot be compiled on a Mac.
- Multi-display placement and mixed-DPI cursors on a Mac.
- The Spotify, WhatsApp, mail and phone link features at run time.
- Flow dictation with real speech models: they download on demand.
- **Bundling:** `tauri build` does not yet copy `coucou-hook` into the app's resources. Dev builds find it in `target/release` (built by `beforeDevCommand`).
