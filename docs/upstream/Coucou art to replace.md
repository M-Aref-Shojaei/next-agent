---
title: Coucou art to replace
tags: [upstream, assets]
status: open
---

# Coucou art to replace

Coucou's code is MIT, but its name, the Mochi character, its icons, sounds and media are not (Coucou's `LICENSE-ASSETS.md`). None of those files is in this repository (`scripts/check-no-coucou-assets.mjs` guards it), and a neutral placeholder stands in where the code needs one. This page lists every spot where the vendored app (`apps/desktop/coucou/`, [[ADR-0011 Vendor Whole Upstream Codebases on Request]]) uses Coucou's artwork, sound or name. Each entry gives the size and format upstream uses, so the owner can supply a replacement.

## 1. The character (Mochi) → placeholder: the gradient orb

Upstream draws Mochi in code on a Canvas 2D (no image files). Mochi's look is the excluded "character", so every drawing of it now draws the orb from `apps/desktop/coucou/src/mochi/orb.ts`. The orb looks like `apps/desktop/src/lib/components/IslandMascot.svelte`: a highlight top-left, a conic gradient of the state's colour, and a glow. Supply a drawing function (or sprite sheets) for each spot:

| Spot | File (function) | Size | States / frames upstream draws |
|---|---|---|---|
| Island mascot | `src/mochi/engine.ts` (`BotEngine.draw`), used by `src/island/island.ts` | canvas W×W; body radius `R = 0.3·W`; in the island the diameter is `VIEW_LAYOUTS[view].botDiameter`, 56–62 CSS px (`src/core/layout.ts`), drawn at 2× DPR | 11 states (`BOT_STATES`: idle, working, thinking, searching, approval, question, error, finished, ratelimit, sleeping, dizzy); 13 eye shapes; 7 emotes; blink, squash, gulp, slap, roll, dance, mailbox morph; hands; blush |
| Badge and particles | `engine.ts` (`drawBadge`, `drawParticles`), **kept** | badge ≈ 0.27 R; hearts, stars, sparks, sweat, "z" | not character art, so upstream's are drawn |
| Mini bots (one per agent pill) | `src/mochi/minibots.ts` → `BotEngine` with `bodyColor` | 14–22 px | flat colour body, state eyes |
| Launch greeting | `src/mochi/greeting.ts` (`drawMochi`, `drawMinis`) | 640×150 reference canvas; body height `HB = 58` | drop, bounce, wave (hands), blink, happy and content eyes, four mini bots |
| Desktop companion window | `src/desktop/main.ts` (route `/coucou/mochi`) | 120×120 window (`PANEL_SIZE`) | same engine; asleep / awake; eyes follow the cursor |
| Dictation pill | `src/pill/main.ts` | about 28 px | engine states while listening or thinking |
| Coucou window header | `src/hub/hero.ts` | about 160 px | idle animation, wardrobe outfit |
| Upload animation | `src/upload/canvas.ts` (`drawMochi`) | diameter `f.d` (about 60 px) in a 640×160 card | body morph into a box, mouth opening, three eye shapes; the file flies into the mouth |
| Weekly recap share image | `src/recap/share.ts` (`drawMochi`) | 1080×1920 PNG export, character about 220 px | idle |
| Outfits (wardrobe) | `src/mochi/outfits.ts`: **not copied**; stand-in draws the orb | wardrobe button about 28×28 px; costumes on the head at R | santa, witch, pumpkin, bunny ears, party hat, crown and the rest (`OUTFIT_SELECTIONS` in `wardrobe.ts`) |

The outfit choice, the seasonal "auto" pick and the wardrobe view still work. Only the costume drawing is missing.

## 2. Icons → placeholder: Next Agent's own icon

| Upstream file (not copied) | Size / format | Used for | Now |
|---|---|---|---|
| `windows/src-tauri/icons/32x32.png` | 32×32 PNG | Windows/Linux bundle | Next Agent's `apps/desktop/src-tauri/icons/icon.png` |
| `windows/src-tauri/icons/128x128.png` | 128×128 PNG | bundle | same |
| `windows/src-tauri/icons/128x128@2x.png` | 256×256 PNG | bundle | same |
| `windows/src-tauri/icons/icon.png` | 512×512 PNG | bundle, window icon | same |
| `windows/src-tauri/icons/icon.ico` | ICO, 7 sizes | Windows installer | none (Next Agent's bundle) |
| `NotchBuddy/Assets.xcassets/AppIcon.appiconset/*` | 16–1024 px PNG (16, 32, 128, 256, 512 @1x and @2x) | Mac app icon | Next Agent's icon |
| `NotchBuddy/Assets.xcassets/MenuBarIcon.imageset/menubar*.png` | template PNG @1x/@2x/@3x (about 18 pt) | Mac menu-bar item | the tray icon uses Next Agent's window icon (`tray.rs`, `app.default_window_icon()`); its "paused" variant is derived from it |
| `windows/scripts/gen-icons.mjs` (copied, code) | generates the PNG/ICO set from a source image | | not run |

## 3. Sounds → placeholder: silence

Upstream keeps 29 WAV files in `NotchBuddy/Resources/sounds/` (16-bit PCM, stereo, 48 kHz). Coucou's Vite config serves them at `/sounds/<name>.wav`; `apps/desktop/coucou/src/core/sound.ts` fetches them from there. Next Agent ships none, so every sound is silent. The user can still drop their own files in Coucou's sounds folder: Settings → Sounds → "Open folder" (`src-tauri/src/sounds.rs`), WAV/MP3/M4A/AIFF, same names.

Names (`SOUND_NAMES` in `sound.ts`):

| Group | Names |
|---|---|
| Island | `peek`, `open`, `close`, `hover`, `blip`, `tick` |
| Character | `slap`, `annoyed`, `dizzy`, `greet`, `greeting`, `gulp`, `love`, `pop`, `proud`, `wink`, `yawn` |
| Agent states | `work`, `think`, `search`, `rate`, `sleep`, `finish`, `error`, `approval`, `question` |
| Actions | `approve`, `send`, `attach` |

To ship replacements, add a `/sounds/` asset folder to the app (an `apps/desktop/static/sounds/` folder served at `/sounds/`) and allow it in the asset guard.

## 4. Media → not copied

| Upstream | What | Now |
|---|---|---|
| `windows/screenshots/*.png` (9 files) | README screenshots | not copied; `apps/desktop/coucou/README.md` links to them and shows broken images |
| `docs/media/*` (21 files) | GIFs and videos of the site | not copied |
| `design/*` (animations, captures, outfits, prototype) | design references | not copied |
| `windows/README.md` line 3 | `<img src="src-tauri/icons/128x128.png">` | broken image |

## 5. The names "Coucou" and "Mochi"

Kept as they are in the vendored code, so it stays a plain diff against upstream. Replace before publishing a build that shows them:

| Where | What | Count / place |
|---|---|---|
| Interface strings | `src/i18n/strings.json`, `src/i18n/extra.json` | "Coucou" ×1160 and "Mochi" ×200 in strings.json, ×882 and ×79 in extra.json, across all languages |
| TypeScript literals | `src/**/*.ts` | "Coucou" ×267, "Mochi" ×203 (comments included) |
| Rust literals | `src-tauri/src/**/*.rs`, `hook/src` | "Coucou" ×520, "Mochi" ×79 (comments included); window titles in `lib.rs` (`create_settings_window`, `create_island_window`), tray menu and title in `tray.rs`, log lines `[coucou]` |
| Window titles of the routes | `apps/desktop/src/routes/coucou/*/+page.svelte` | "Coucou", "Mochi" |
| What's new | `src/whats-new/whats-new.json` | release notes naming Coucou |
| Identifiers, not shown but carrying the name | keychain service `fr.louisraille.coucou` (`secrets.rs`); relay binary `coucou-hook` and its entries in `~/.claude/settings.json` (`hooks.rs`); socket `coucou.sock`; mDNS `_coucou._tcp` (`phone_link/discovery.rs`); relay label `coucou-relay/v1`; pill ids and `coucou_*` hook fields | changing them breaks compatibility with Coucou's relay, phone app and hooks; change them together |
| Update check | `src-tauri/src/update.rs` | asks `github.com/Louis-CFM/coucou` releases, and only after the user agrees |
| Next Agent's own UI | Settings → "Overlay / Coucou" (`apps/desktop/src/routes/(app)/settings/+page.svelte`) | credit line |
