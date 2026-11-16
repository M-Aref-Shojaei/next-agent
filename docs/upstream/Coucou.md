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
| Settings and the hub window | `settings.rs`, `src/hub` (`src/settings` is removed, see "Deviations requested by the owner") |
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
  - Files and ids: the relay `next-agent-hook`, the log (retired: see the overlay log audit below), the socket `relay.sock` (Mac) / `next-agent.sock` (Linux), the pipe `next-agent-<sid>`, `NEXT_AGENT_*` environment variables, the Keychain service `dev.nextagent.desktop.overlay`, the portal app id `dev.nextagent.desktop`, the data folders `…/dev.nextagent.desktop/overlay` on every system, the wake phrase « OK Next Agent » (Coucou's spellings still wake it).
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
- **Removed: Coucou's settings code.** `src/settings/*` (`main.ts`, `agents.ts`, `phone.ts`, `updates.ts`, `colors.ts`, `parts.ts`, `settings.css`), `src/hub/pages.ts`, `hub/pages/{flowsettings,styles,voice}.ts`, the tests of the removed pages (`phone-pairing-hint`, `phone-relay`, and the Flow settings and styles cases of `flow-history` and `flow-editors`) and the 200 `i18n/extra.json` strings that only they used (the i18n test fails on an unused string). Why: they were unused, and `settings.css` was the last raw-value stylesheet the design-token checker skipped (ADR-0012). A re-sync takes upstream's settings changes as a reference for Settings → Overlay, not as files.
- **Moved, behaviour unchanged.** `claude-models.ts` is now `src/core/claude-models.ts` (Settings → Chat imports its `CLAUDE_MODELS` list as `$coucou/core/claude-models`). `modelsControl` (voice model download, progress, installed) is now `src/hub/models.ts`; the dictation page and the Notetaker use it.
- **The Coucou window's base styles** are `src/hub/base.css`: only the `settings.css` rules that reach this window (the `*` reset, buttons, the switch, the status dot, the spacer, a text field's font and colour, the select popup), on the design tokens (`--font` is `--font-sans`; fills, text, ok colour, radii and transition times are tokens). Sizes are Coucou's. The window keeps its label `coucou-settings` and route `/coucou/settings` (it is the hub window: `HUB_LABEL`), so its capability file stays; it lost `flow_clear` and `flow_paste_again`, which only the removed Flow settings page called.
- **Who may change what.** The phone link's and Antigravity's "only the settings window" checks now name the main window (`SETTINGS_LABEL = "main"`); the Coucou window no longer passes them. Capabilities: `main` gets the 70 overlay settings commands (pinned in `scripts/tests/capabilities.test.ts`); `coucou-settings` loses the settings writers (`hooks_apply`, `status_line_apply`, `agent_hooks_apply`, `secret_*`, `local_set_key`, `save_settings`, the phone switches, `update_set_source` …). Overlay windows still get none of Next Agent's commands. The CSP is unchanged.

## Overlay log audit (ADR-0013, 2026-11-16)

Coucou's `log::line(...)` wrote free text to `overlay/next-agent.log`. It now emits `tracing` events with target `overlay` (`src-tauri/src/log.rs`); the desktop app's subscriber writes them to the shared `logs/` folder (docs/architecture/Logging.md). The old file is deleted at start (`log::retire_old_file`). The rule is the owner's: log how the app behaves, never what people work on.

- **Helpers** (`log.rs`): `line(event, text)` for safe constant text (the text goes to the redacted `reason`); `fail(event, error)` for a failure (the redacted `error`); `ui_note(text)` for the island's `log_line` command. Call sites that carry values use `tracing::info!/warn!(target: "overlay", ...)` with allowlisted fields: `hook_event`, `relay_connection`, `overlay_window`, `dictation`, `phone_link`, `overlay_setting`, `update_*`, `shortcut*`, and so on.
- **Where the old message could carry user content, the call site no longer logs the text.** It logs an event name and a constant reason, or allowlisted fields (status, count, duration_ms, kind, approval_id).
- **Rule of thumb for `error`:** only errors from the operating system, our own code or the network layer. An error that comes from a provider's response (Claude API detail), a mail client, or a transcript pipeline is replaced by a constant.
- **The island's `log_line` command** takes free text from the web page; one TypeScript call (`island.ts`, the drop line) sends a session name. The Rust side keeps only a leading lowercase word as `kind` and drops the rest, so no TypeScript change is needed (`apps/desktop/coucou/src` is left to the UI task).
- **`next-agent-hook`** (the relay, `apps/desktop/coucou/hook`) writes no log and no file: it reads the hook JSON on stdin, writes to the pipe or socket and to stdout, and reads the saved status line file. It has no `tracing`, no `eprintln!` and no file creation, so there is nothing to change.
- **Settings and recap file messages** (`settings.rs:244`, `recap.rs:461`) still pass their text to `reason`: it names a settings key, an error and a path. The redactor shortens a path to its depth and extension and a home folder to `~`, and none of it is user content.
- **Dropped:** the dictation key label, the Claude endpoint host, the folders searched for the relay, workflow names, project folder names, GraphQL error text and file paths in the drop messages; none of them is a field on the allowlist, and most are user content or private addresses.
- **Windows and Linux files were edited but not compiled here** (`webview_drop.rs`, `platform/linux*.rs`, `flow/wayland.rs`, `portal.rs`).
- **Wave 3 (2026-11-16, [[Logging]] "Audit 2026-11-16"):** the windows' `call` helpers (`core/flow.ts`, `core/bridge.ts`, `core/mail.ts`) log `overlay_command_failed` with the command name instead of `console.error`, and `emitToWindow` logs `overlay_emit_failed`, through Next Agent's `lib/log.ts` and `diag_log`; only session-level `hook_event`s stay `info`, the per-tool-call ones (`PreToolUse`/`PostToolUse`/`PostToolUseFailure`) and unlisted kinds are `debug` (`pipe.rs`); `island_notch` logs `notch` or `no_notch` (`island.rs`). All marked `// Next Agent:`.

**Totals:** 157 calls audited, 157 changed to a constant event name or allowlisted fields (every one, since each now has an event name), 12 carried user content or private values (the high-risk ones, marked below). The rest of the old messages were safe, mostly counts, ids and OS errors.

Risk: *none* means the old message held only constants, counts, ids, versions or system errors. *user content* means it could carry what someone works on, or a private address. Line numbers are the call's line in the file before this change.

| File:line | Old message | Risk | New event |
|---|---|---|---|
| `agy_approvals.rs:126` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `agy_approvals.rs:130` | `format!("Antigravity approvals from the island {}", if enabled { "on" } else { "off" })` | none | `overlay_setting` |
| `claude.rs:62` | `format!( "chat: Claude requests go to {} ({BASE_URL_VAR})", net::host_for_log(url) )` | user content: endpoint host (a private address) | `claude_endpoint_overridden` |
| `claude.rs:66` | `format!("chat: {err}")` | user content: the error echoes the configured URL | `claude_endpoint_invalid` |
| `desktop.rs:292` | `format!("desktop Next Agent: {}", mode.as_str())` | none | `desktop_mode` |
| `desktop.rs:343` | `format!("desktop Next Agent window failed: {err}")` | none | `desktop_window_failed` |
| `desktop.rs:567` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `desktop.rs:828` | `"desktop Next Agent: his spot is on a display that is gone — he stays home"` | none | `desktop_position_reset` |
| `flow/cleanup.rs:42` | `format!("flow: clean-up skipped: {err}")` | user content: Claude API error detail can echo the dictation | `dictation_cleanup_skipped` |
| `flow/mod.rs:167` | `format!("flow: the key can't be watched: {err}")` | none | `dictation_key_unwatchable` |
| `flow/mod.rs:174` | `format!("flow: no key on this desktop ({why}); the pill's buttons still work")` | none | `dictation` |
| `flow/mod.rs:188` | `format!("flow dictation on ({})", prefs.key.label(&prefs.key_combo))` | none | `dictation` |
| `flow/mod.rs:355` | `format!("flow: microphone: {err}")` | none | `dictation_microphone_failed` |
| `flow/mod.rs:394` | `format!("flow: {err}")` | none | `dictation_failed` |
| `flow/mod.rs:422` | `format!("flow: {} words in {:?}", text::word_count(&final_text), started.elapsed())` | none | `dictation` |
| `flow/paste.rs:66` | `format!("flow: paste keystroke failed: {err}")` | none | `dictation_paste_failed` |
| `flow/pill.rs:99` | `"flow pill: no way to place it on this desktop"` | none | `dictation_pill_unplaced` |
| `flow/pill.rs:136` | `format!("flow pill window failed: {err}")` | none | `dictation_pill_failed` |
| `flow/store.rs:240` | `format!("flow: could not save {name}: {err}")` | none | `dictation_store_failed` |
| `flow/wayland.rs:45` | `"flow: dictation shortcut held by the desktop"` | none | `dictation_shortcut_bound` |
| `flow/wayland.rs:47` | `format!("flow: no dictation shortcut from the desktop ({reason})")` | none | `dictation` |
| `herdr.rs:113` | `format!("herdr focus failed: {e}")` | none | `herdr_focus_failed` |
| `hooks.rs:296` | `"moved statusline-previous.json out of bin/"` | none | `hook_cleanup` |
| `hooks.rs:435` | `format!( "{} not found — Claude Code hooks cannot work. Looked in: {}", platform::HOOK_EXE, tried.join(", ") )` | user content: lists the folders searched (paths) | `hook_binary_missing` |
| `hooks.rs:458` | `format!("could not install {}: {err}", platform::HOOK_EXE)` | none | `hook_install_failed` |
| `hooks.rs:479` | `format!("could not install {}: {err}", platform::HOOK_EXE)` | none | `hook_install_failed` |
| `integrations.rs:793` | `format!("github {what}: no connection")` | none | `github_request_failed` |
| `integrations.rs:797` | `format!("github {what} HTTP {}", response.status().as_u16())` | none | `github_request_failed` |
| `integrations.rs:803` | `format!("github {what} GraphQL errors: {errors}")` | none | `github_request_failed` |
| `integrations.rs:1173` | `format!("n8n list HTTP {}", response.status())` | none | `n8n_request_failed` |
| `integrations.rs:1224` | `format!("n8n execution {id} {status} · {name}")` | high: user content: workflow name | `n8n_execution` |
| `island.rs:90` | `format!("island notch: {notch:?}")` | none | `island_notch` |
| `island.rs:520` | `format!( "window size mismatch: wanted {pw}x{ph}, got {}x{} (scale {scale}, monitor {}x{})", got.width, got.he` | none | `island_size_mismatch` |
| `island.rs:596` | `"display layout changed — repositioning".to_string()` | none | `island_display_changed` |
| `launch.rs:265` | `format!("launch: {candidate} didn't start: {err}")` | none | `terminal_launch_failed` |
| `launch.rs:297` | `format!("launch: Windows Terminal didn't start: {err}")` | none | `terminal_launch_failed` |
| `launch.rs:355` | `format!("launch: Claude Code in {name} with {terminal}")` | high: user content: project folder name | `terminal_launch` |
| `launch.rs:359` | `format!("launch: couldn't open a terminal in {name}: {err}")` | user content: project folder name in the message | `terminal_launch_failed` |
| `launch.rs:393` | `format!("drop: {} file(s) into a session, window found: {focused}", paths.len())` | none | `file_drop` |
| `lib.rs:130` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `lib.rs:235` | `format!("webview dpr {dpr} != window scale {scale} — setting zoom {new_zoom:.3}")` | none | `webview_zoom_adjusted` |
| `lib.rs:445` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `lib.rs:472` | `format!("agent hooks {done} for {agent}")` | none | `agent_hooks_changed` |
| `lib.rs:503` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `lib.rs:645` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `lib.rs:714` | `format!("ui {message}")` | high: user content: free text sent by the island (one call sends a session name) | `ui_note` |
| `lib.rs:780` | `"Next Agent's main window is missing"` | none | `overlay_window` |
| `lib.rs:824` | `format!("Next Agent window failed: {err}")` | none | `overlay_window` |
| `lib.rs:867` | `"Next Agent window missing, creating it again"` | none | `overlay_window` |
| `lib.rs:1209` | `format!("island window failed: {err}")` | none | `overlay_window` |
| `lib.rs:1238` | `format!("--- Next Agent {} started ---", env!("CARGO_PKG_VERSION"))` | none | `overlay_started` |
| `mail/mod.rs:184` | `format!("[mail] Outlook: {e}")` | user content: the error can carry mail content | `mail_outlook_failed` |
| `mail/store.rs:149` | `format!("[mail] could not save {name}: {e}")` | none | `mail_store_failed` |
| `notes/mod.rs:292` | `format!("notes: microphone: {err}")` | none | `notes_microphone_failed` |
| `notes/mod.rs:304` | `format!("notes: system sound unavailable: {err}")` | none | `notes_system_sound_unavailable` |
| `notes/mod.rs:327` | `"notes: recording"` | none | `notes_recording` |
| `notes/mod.rs:351` | `format!("notes: stopped by itself ({why:?})")` | none | `notes_auto_stopped` |
| `notes/mod.rs:469` | `format!("notes: transcript failed: {err}")` | user content: the error comes from the transcript pipeline | `notes_transcript_failed` |
| `notes/mod.rs:530` | `format!("notes: transcript of {count} lines")` | none | `notes_transcript` |
| `phone_link/chat_backend.rs:69` | `format!("phone link: chat via {provider} failed ({kind:?})")` | none | `phone_chat_failed` |
| `phone_link/mod.rs:69` | `format!("phone link: answered a question id={request_id}")` | none | `phone_link` |
| `phone_link/mod.rs:78` | `format!("phone link: decision id={request_id} {word}")` | none | `phone_link` |
| `phone_link/mod.rs:168` | `"phone link: relay address not valid, not connecting"` | none | `phone_link` |
| `phone_link/mod.rs:172` | `"phone link: relay needs an access key, not connecting"` | none | `phone_link` |
| `phone_link/mod.rs:176` | `"phone link: connecting to the relay"` | none | `phone_link` |
| `phone_link/mod.rs:190` | `format!("phone link: listening on port {}", r.port)` | none | `phone_link` |
| `phone_link/mod.rs:193` | `"phone link: announced on the local network"` | none | `phone_link` |
| `phone_link/mod.rs:194` | `format!("phone link: not announced on the local network: {e}")` | none | `phone_link` |
| `phone_link/mod.rs:203` | `format!("phone link: could not start: {e}")` | none | `phone_link` |
| `phone_link/mod.rs:240` | `"phone link: stopped"` | none | `phone_link` |
| `phone_link/mod.rs:314` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:376` | `"phone link: new pairing code, phones disconnected"` | none | `phone_link` |
| `phone_link/mod.rs:447` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:452` | `format!("phone link: relay {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:474` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:483` | `format!("phone link: relay access key changed, {told} phone(s) told")` | none | `phone_link` |
| `phone_link/mod.rs:517` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:522` | `format!("phone link: details {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:555` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:560` | `format!("phone link: answering questions {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:593` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:598` | `format!("phone link: file changes on the phone {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:631` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:636` | `format!("phone link: plan usage on the phone {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:669` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:674` | `format!("phone link: weekly recap on the phone {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:710` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:715` | `format!("phone link: {} service(s) on the phone", allowed.len())` | none | `phone_link` |
| `phone_link/mod.rs:830` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `phone_link/mod.rs:839` | `format!("phone link: chat {}", if enabled { "on" } else { "off" })` | none | `phone_link` |
| `phone_link/mod.rs:858` | `format!("could not save settings: {err}")` | none | `settings_save_failed` |
| `pipe.rs:68` | `"too many relay connections at once; one refused"` | none | `relay_connection` |
| `pipe.rs:111` | `format!("cannot open the relay pipe: {err}")` | none | `relay_connection` |
| `pipe.rs:128` | `format!("cannot reopen the relay pipe: {err}")` | none | `relay_connection` |
| `pipe.rs:136` | `"refused a relay connection from another user"` | none | `relay_connection` |
| `pipe.rs:159` | `"no private runtime directory ($XDG_RUNTIME_DIR) — Claude Code hooks are inactive"` | none | `relay_connection` |
| `pipe.rs:167` | `"another Next Agent already serves the relay socket"` | none | `relay_connection` |
| `pipe.rs:175` | `format!("cannot open the relay socket: {err}")` | none | `relay_connection` |
| `pipe.rs:202` | `"refused a relay connection from another user"` | none | `relay_connection` |
| `pipe.rs:255` | `"dropped a relay connection that sent no complete line in time"` | none | `relay_connection` |
| `pipe.rs:287` | `format!("hook {event}")` | none | `hook_event` |
| `pipe.rs:302` | `format!("hook PermissionRequest id={id}")` | none | `hook_event` |
| `pipe.rs:311` | `format!("hook id={id} answered elsewhere")` | none | `hook_event` |
| `pipe.rs:344` | `"hook PreToolUse"` | none | `hook_event` |
| `pipe.rs:360` | `format!("hook Antigravity command waits for the island id={id}")` | none | `hook_event` |
| `pipe.rs:380` | `format!("hook id={id} timed out — Antigravity asks")` | none | `hook_event` |
| `pipe.rs:402` | `format!("hook id={id} answered {word}")` | none | `hook_event` |
| `pipe.rs:413` | `format!("hook id={id} {why}")` | none | `hook_event` |
| `pipe.rs:466` | `format!("hook id={id} answered {}", loggable(&d))` | none | `hook_event` |
| `pipe.rs:470` | `format!("hook id={id} not shown — terminal takes over")` | none | `hook_event` |
| `pipe.rs:475` | `format!("hook id={id} island never acknowledged — terminal takes over")` | none | `hook_event` |
| `pipe.rs:482` | `format!("hook id={id} answered {}", loggable(&d))` | none | `hook_event` |
| `pipe.rs:486` | `format!("hook id={id} released without a decision")` | none | `hook_event` |
| `pipe.rs:490` | `format!("hook id={id} timed out — terminal takes over")` | none | `hook_event` |
| `pipe.rs:511` | `format!("reply for id={request_id} — no pending request")` | none | `hook_event` |
| `pipe.rs:546` | `format!("decline id={request_id}")` | none | `hook_event` |
| `pipe.rs:557` | `format!("decision id={request_id} {word}")` | none | `hook_event` |
| `pipe.rs:585` | `format!("decision id={request_id} answered a question")` | none | `hook_event` |
| `platform/linux.rs:522` | `format!("island is a regular window ({why})")` | none | `overlay_window` |
| `platform/linux.rs:573` | `"island is a layer-shell overlay"` | none | `overlay_window` |
| `platform/linux.rs:774` | `"desktop Next Agent: window already shown, no layer surface"` | none | `overlay_window` |
| `platform/linux_focus.rs:157` | `format!("open terminal: window raised ({method:?})")` | none | `terminal_focus` |
| `platform/macos.rs:319` | `format!("island panel: {err}")` | none | `island_panel_failed` |
| `portal.rs:378` | `format!("shortcuts portal: no app registry ({err})")` | none | `shortcuts_portal_unavailable` |
| `portal.rs:389` | `format!("shortcuts portal: GlobalShortcuts version {version}")` | none | `shortcuts_portal` |
| `recap.rs:461` | `message` | none | `recap_file_issue` |
| `settings.rs:244` | `message` | none | `settings_file_issue` |
| `shortcuts.rs:325` | `format!("shortcut {action}")` | none | `shortcut` |
| `shortcuts.rs:337` | `format!("shortcuts: unregister failed: {err}")` | none | `shortcut_unregister_failed` |
| `shortcuts.rs:379` | `format!("shortcuts: {} not registered: {err}", def.id)` | none | `shortcut_register_failed` |
| `shortcuts.rs:543` | `"shortcuts: registered with the desktop portal"` | none | `shortcuts_portal` |
| `shortcuts.rs:549` | `format!("shortcuts: the desktop portal didn't take them: {reason}")` | none | `shortcuts_portal` |
| `shortcuts.rs:554` | `format!("shortcuts: no portal, commands instead ({reason})")` | none | `shortcuts_portal` |
| `speech/audio.rs:342` | `format!("audio: {err}")` | none | `audio_stream_error` |
| `speech/mod.rs:195` | `"speech models downloaded"` | none | `speech_models_downloaded` |
| `speech/mod.rs:198` | `format!("speech models download failed: {err}")` | none | `speech_models_download_failed` |
| `speech/mod.rs:309` | `format!("Parakeet loaded in {:?}", started.elapsed())` | none | `speech_model_loaded` |
| `speech/mod.rs:310` | `"Parakeet could not be loaded"` | none | `speech_model_load_failed` |
| `speech/mod.rs:441` | `"diarizer could not be loaded"` | none | `speech_model_load_failed` |
| `update.rs:435` | `format!("update: could not save update.json: {err}")` | none | `update_state_save_failed` |
| `update.rs:618` | `format!("update: GitHub unreachable: {err}")` | none | `update_check_failed` |
| `update.rs:624` | `format!("update: {} is out (running {})", release.version, current_version())` | none | `update_available` |
| `update.rs:684` | `format!("update: running the {kind:?} installer for {}, quitting", release.version)` | none | `update_installing` |
| `update.rs:691` | `format!("update: failed: {message}")` | none | `update_failed` |
| `update.rs:726` | `format!("update: could not start the installer: {err}")` | none | `update_installer_failed` |
| `voice/brain.rs:116` | `format!("voice: {err}; the chat's model instead")` | user content: Claude API error detail | `voice_model_fallback` |
| `voice/brain.rs:125` | `format!("voice: Claude turn failed: {err}")` | user content: Claude API error detail | `voice_turn_failed` |
| `voice/mod.rs:74` | `"voice: « OK Next Agent » listening"` | none | `voice_wake` |
| `voice/mod.rs:84` | `"voice: « OK Next Agent » off"` | none | `voice_wake` |
| `voice/mod.rs:125` | `format!("voice: microphone: {err}")` | none | `voice_microphone_failed` |
| `voice/mod.rs:181` | `"voice: « OK Next Agent » heard"` | none | `voice_wake` |
| `voice/mod.rs:337` | `format!("voice: ElevenLabs {}", response.status())` | none | `voice_request_failed` |
| `webview_drop.rs:39` | `format!("file drop: no webview: {err}")` | none | `file_drop_failed` |
| `webview_drop.rs:66` | `format!("file drop: cannot listen: {err}")` | none | `file_drop_failed` |
| `webview_drop.rs:70` | `format!("file drop: {err}")` | none | `file_drop_failed` |
| `webview_memory.rs:31` | `format!("webview memory {label}: {err}")` | none | `webview_memory` |
| `webview_memory.rs:35` | `format!("webview memory: with_webview failed: {err}")` | none | `webview_memory` |

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
