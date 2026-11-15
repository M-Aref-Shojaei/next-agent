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
| ~~WhatsApp (GOWA)~~, ~~Spotify~~ | **removed** at the owner's request (see below) |
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
  - Per-window command access. `apps/desktop/src-tauri/build.rs` declares every command (Next Agent's and the overlay's) in Tauri's app manifest, so a window may call only what its capability grants. `scripts/gen-capabilities.mjs` writes the capabilities from the code: `default.json` gives `main` Next Agent's commands plus exactly the overlay settings commands that `apps/desktop/src/lib/overlay-settings.ts` calls (no other file of Next Agent's frontend may name an overlay command); `overlay-{island,settings,mochi,pill}.json` give each overlay window the overlay commands its own page reaches (its imports, the bridge methods they call, the commands those invoke), and none of Next Agent's. `make check` fails when they are stale; `scripts/tests/capabilities.test.ts` pins the rules.
  - CSP: Next Agent's, plus upstream's `media-src 'self' blob:`, `object-src 'none'`, `base-uri 'self'`, `form-action 'self'`, `frame-ancestors 'none'`.
- **macOS.**
  - The island webview moves into the `IslandPanel` (level `mainMenu + 3`, `constrainFrameRect` passthrough), placed at the top centre of the chosen display by its frame in points.
  - The cursor poll flips `ignoresMouseEvents`.
- **Next Agent's tasks.**
  - `apps/desktop/src-tauri/src/coucou.rs` turns each `task-stream` event into the hook event Claude Code would send: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`/`PostToolUseFailure`, `Stop`, `StopFailure`, `Interrupt`.
  - Each event is tagged `coucou_agent: "next-agent"`, which gives Next Agent its own pill. It is written to `pipe.rs` `connect_in_process`, the same `handle` a relay connection goes through.
  - The island's Allow/Deny comes back on that stream and goes to `approval_respond`.
  - An approval answered in the main window closes the stream, and the card goes.
- **Claude Code.**
  - The relay `next-agent-hook` and its socket `~/Library/Application Support/dev.nextagent.desktop/overlay/relay.sock`: a 0700 folder, and a uid check on both ends (on Windows, the pipe `\\.\pipe\next-agent-<sid>`, the client's SID checked by the app too).
  - Limits: the first line within 5 s, at most 1 MiB (larger is refused, not cut), at most 64 connections at once; the relay reads at most 32 MiB of stdin.
  - Hooks are written only from Settings → Overlay → Agents (Claude Code), after its diff, dated backup and confirmation in a dialog (with the fingerprint of the preview). Nothing is written until then; of the overlay's windows none may call the write commands, only the main window.
  - The island offers Allow (once) and Deny; only those reach `approval_respond`. TODO(channels): hide any future "Always" for tasks the stream marks as from a channel.
- **Settings.** The overlay's settings are Next Agent's Settings → Overlay (`/settings/overlay/*`); see "Deviations requested by the owner" below.
- **Self-update.** Settings → General → Updates → "Update source" takes a GitHub `owner/repo`, empty by default; while empty nothing is checked or asked. Once set, the upstream consent question and schedule apply, tags are `vX.Y.Z`, and only releases and assets of that repository count.

### Differences from upstream (each marked `Next Agent:` in the code)
- **Not at launch:**
  - Coucou's window does not open at launch.
  - Single-instance, autostart and global-shortcut plugins come with it.
  - `tauri-build`'s `cfg(dev)` is never set in the library.
- **The character:** drawn as the orb (`orb.ts`). `outfits.ts` keeps only the wardrobe icon.
- **Tests:**
  - Tests that read Coucou's Mac app or web site are skipped when those files are absent.
  - Two upstream tests shared a temp folder and raced on macOS; one now uses its own.
  - Under `cargo test` the home folder is a temp folder of the test process, `CLAUDE_CONFIG_DIR` and the XDG / `%APPDATA%` variables are ignored, and secrets go to keyring's mock store: no test touches the real Application Support, `~/.claude` or Keychain.
- **WhatsApp and Spotify: removed** (owner request). Code, UI, pills, settings, commands, the voice "music" tool, the dance to music, i18n strings, What's new items, tests, dependencies (`miniz_oxide`, the WinRT media and registry features) and the three `whatsapp-*` Keychain entries (deleted on first run on a Mac). Each cut is marked `Next Agent: removed WhatsApp/Spotify (owner request)`. JSON has no comments: the What's new items and i18n strings that named them were dropped.
- **Renamed to Next Agent** (owner request; marked `Next Agent: renamed from Coucou (owner request)` where code changed):
  - Visible text: inside string literals and the JSON tables only, "Coucou" and "Mochi" (and their Korean, Hindi, Arabic, Bengali and Russian spellings) became "Next Agent", "Coucoudoro" "Pomodoro". Comments, identifiers and the attribution headers are upstream's. A re-sync applies the same rule to the new upstream strings.
  - Files and ids: the relay `next-agent-hook`, the log `next-agent.log`, the socket `relay.sock` (Mac) / `next-agent.sock` (Linux), the pipe `next-agent-<sid>`, `NEXT_AGENT_*` environment variables, the Keychain service `dev.nextagent.desktop.overlay`, the portal app id `dev.nextagent.desktop`, the data folders `…/dev.nextagent.desktop/overlay` on every system, the wake phrase « OK Next Agent » (Coucou's spellings still wake it).
  - First run on a Mac: the old `dev.nextagent.desktop/coucou` folder moves to `overlay` (a link stays at the old path, so hooks written before keep starting and exit quietly), and the Keychain entries under `fr.louisraille.coucou` move to the new service (marker `.keychain-migrated`). Hooks with the old relay path still count as ours, so Install replaces them and Remove takes them out. On Windows and Linux nothing is moved: those old names belong to Coucou's own app.
  - Kept, on purpose: the source folder, crate and module names (`apps/desktop/coucou`, `coucou_lib`, `coucou.rs`, `$coucou`), window labels and routes (`coucou-*`, `/coucou/*`), the hook payload fields (`coucou_agent` …), browser storage keys, other agents' plugin file names and group key (`coucou.json`, `coucou.js`, `"coucou"`), and the phone link's wire names (`_coucou._tcp`, `coucou://pair`, `coucou.v1`, `coucou-relay/v1`), which Coucou's Android app must match.
- **Lints and checks:**
  - Upstream's style lints are allowed by name, and its formatting is kept (`rustfmt.toml`).
  - The structure heuristic skips the folder.

## Deviations requested by the owner

### One design system: the settings live in Next Agent's Settings (2026-11-15)

The owner's rule ([[ADR-0012 One Design System]]): one design system everywhere, and every built-in upstream's settings inside Next Agent's own Settings.

- **Where.** Settings → Overlay, one page per section, built from `SettingsLayout`, `SettingsSection` and `SettingsRow`:

  | Section | Route | Coucou source |
  |---|---|---|
  | General | `/settings/overlay/general` | `settings/main.ts` generalSection, recapRows, languageRow |
  | Pills and colours | `/settings/overlay/pills` | activePillsSection, `colors.ts` |
  | Agents | `/settings/overlay/agents` | claudeSection, planSection, `agents.ts` |
  | Chat | `/settings/overlay/chat` | subscriptions, apiSection, `claude-models.ts`, chat providers, local models |
  | Integrations | `/settings/overlay/integrations` | integrationsSection |
  | Dictation | `/settings/overlay/dictation` | `hub/pages/flowsettings.ts` (keys, microphone, recognition, privacy), `hub/pages/styles.ts` |
  | Notetaker | `/settings/overlay/notetaker` | flowsettings' Notetaker card |
  | Voice | `/settings/overlay/voice` | `hub/pages/voice.ts` |
  | Shortcuts | `/settings/overlay/shortcuts` | shortcutsSection |
  | Phone link | `/settings/overlay/phone` | `phone.ts` |
  | Updates | `/settings/overlay/updates` | `updates.ts` |
  | About | `/settings/overlay/about` | the window's brand and version line; credits from NOTICE.md, no artwork |

- **Same behaviour.** Same commands, arguments, defaults, validation and order of operations (`apps/desktop/src/lib/overlay-settings.ts` mirrors `core/bridge.ts` and `core/flow.ts`, including which calls swallow errors and which surface them). The settings writers keep the preview, the dated backup and the explicit confirmation, now in the design system's `Dialog`; the write carries the preview's fingerprint. In a plain browser the calls answer from `mock-api.ts`.
- **Small differences, visual only.** Texts are English (Next Agent's Settings are); the island, tray and hub still follow the Language setting. The colour palette opens inline under the dot. Flow's "Open this window when you open Next Agent" is not shown: the Coucou window never opens at launch in Next Agent, so it did nothing.
- **Entry points.** Rust's `show_hub` sends the moved pages (`MOVED_PAGES` in `src-tauri/src/lib.rs`) to the main window instead (`overlay-settings-open` event, `lib/app.svelte.ts` navigates). So the tray's "Settings…", the island's settings links (`open_settings_window`) and the Flow pill's "settings" open Settings → Overlay. Next Agent's own `overlay_open` command is gone.
- **The Coucou window stays, without its settings.** It keeps the Flow pages that are not settings (dictation history, the Notetaker, statistics, dictionary, snippets, mail). Its new entry `src/hub/main.ts` is `settings/main.ts`'s `main()` without the Notch pages; `hub/shell.ts` takes an `elsewhere` hook, so its sidebar and search links to settings pages open Settings → Overlay (`Bridge.openSettingsWindow(page)`).
- **Kept unused.** `src/settings/main.ts`, `agents.ts`, `phone.ts`, `updates.ts`, `colors.ts`, `claude-models.ts` (only its `CLAUDE_MODELS` list is imported), `parts.ts`, and `hub/pages/{flowsettings,styles,voice}.ts` as pages (`flowsettings.ts`'s `modelsControl` is still used by the dictation page). `settings/settings.css` still gives the Coucou window its base styles. Removing them would be a large diff against upstream; a re-sync stays a plain diff.
- **Who may change what.** The phone link's and Antigravity's "only the settings window" checks now name the main window (`SETTINGS_LABEL = "main"`); the Coucou window no longer passes them. Capabilities: `main` gets the 70 overlay settings commands (pinned in `scripts/tests/capabilities.test.ts`); `coucou-settings` loses the settings writers (`hooks_apply`, `status_line_apply`, `agent_hooks_apply`, `secret_*`, `local_set_key`, `save_settings`, the phone switches, `update_set_source` …). Overlay windows still get none of Next Agent's commands. The CSP is unchanged.

## Upstream watch

- Settings → Upstreams lists what Coucou published since the pin: commits (title, date, scope, link), releases, and the areas the range changed.
- `crates/core/src/upstream/` reads the `watch` entry of `upstream.lock.json` (embedded at build time). It asks GitHub's public API once a day while the app runs, or on "Check now". It makes three unauthenticated requests: repository, compare from the pin, and releases.
- It sends no token and no user data. A rate limit (403/429 with GitHub's headers) is remembered, and no check runs until it ends. The last result is kept in `upstream-watch.json` in the app data folder.
- It never applies anything. Bringing a change in is a manual re-sync from the pin, then a re-pin of both `pin` and `watch.commit`.
- It is generic: another upstream joins by getting a `watch` entry.

## Not verified
- The Windows and Linux builds of the vendored crate: they cannot be compiled on a Mac.
- Multi-display placement and mixed-DPI cursors on a Mac.
- The mail and phone link features at run time.
- The Windows pipe's SID check and the other Windows/Linux edits of this change: written, not compiled here.
- Flow dictation with real speech models: they download on demand.
- **Bundling:** `tauri build` does not yet copy `next-agent-hook` into the app's resources. Dev builds find it in `target/release` (built by `beforeDevCommand`).
