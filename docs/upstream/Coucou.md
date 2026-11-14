---
title: Coucou
tags: [upstream]
status: researched
---

# Coucou — what we port

- Repo: https://github.com/Louis-CFM/coucou
- Licence: **MIT for code only**, © 2026 Louis Raillé.
- **Do not reuse** the name "Coucou", the Mochi character, the icon, the sounds or any other media. `LICENSE-ASSETS.md` excludes them from the licence.
- Platforms: macOS is Swift/SwiftUI (`NotchBuddy/`). Windows and Linux use **Tauri 2** (`windows/`), the same stack as ours.

## Our overlay
- A second Tauri window.
- Shows the active task, the mode, and pending approvals. You can approve or deny from it.
- Reads task events directly inside the app. It needs **no hooks, sockets or Claude settings edits**; Coucou needs those only because it watches outside agents.

## Window setup (from `windows/src-tauri/tauri.conf.json`, `island.rs`)
- Window flags: `decorations:false`, `transparent:true`, `alwaysOnTop:true`, `skipTaskbar:true`, `shadow:false`, `focus:false`.
- Size: start tiny (240×6). Grow to about 720×320 when expanded.
- Placement:
  1. Centre horizontally on the chosen monitor.
  2. On Windows, offset below the work-area top so a top taskbar is not covered.
  3. Call `set_size` → `set_position` → `set_size` again (a move to another monitor can rescale the window) → `set_always_on_top(true)`.
  4. On Linux, call `set_resizable(true)` first. GTK won't shrink a non-resizable window.
- Monitor choice: a saved display ID. If it is missing, use the cursor's display, then the primary display.

## Click-through
- Windows/macOS: poll the cursor position (16 ms when near the island, 50 ms when far). Call `set_ignore_cursor_events` only when the state changes. Add a 14 px margin around the island.
- macOS (Coucou native): adaptive polling, from 120 Hz while dragging down to 8 Hz when idle. Pause while the display sleeps.
- Linux: set an input region shaped like the island. On wlroots and KDE, use gtk-layer-shell. On GNOME, run as an XWayland dock window.

## macOS notch (needs objc2 calls on `ns_window()`, or the third-party `tauri-nspanel`, which is unverified)
- Use an `NSPanel` with `.nonactivatingPanel`, at level `mainMenu+3`.
- Collection behaviour: `canJoinAllSpaces`, `stationary`, `fullScreenAuxiliary`, `ignoresCycle`.
- Override the frame constraint so the panel can sit in the notch.
- Detect the notch from `NSScreen.safeAreaInsets.top > 0`.
  - Notch width = screen width − left auxiliary area − right auxiliary area (fallback 184 pt).
  - No notch: draw an 80 pt pill, at most 24 pt tall.
- No private APIs were seen in the code we read.

## Animation
- Grow: a spring (response 0.5, damping 0.72).
- Shrink: `cubic-bezier(0.45, 0, 0.2, 1)` over 0.34 s.

## Approval queue (`ApprovalQueue.swift`)
- First in, first out. Holds at most 5 approvals.
- Each approval has a timeout of about 115 s.
- An approval is cleared when its session stops, its session sends a new prompt, or its session ends.
- An approval matches its result by (session, tool, input key).

## Implementation status (overlay: a port of Coucou)

Ported from Coucou `0d7fe58884d99f275f3238e8dbe29ddee69fa9e8` (pinned in `upstream.lock.json`). Every ported file starts with `// Ported from Coucou (MIT, © 2026 Louis Raillé): <path> @ <commit>`. Code only: no name, character, icon, sound or media. `scripts/check-no-coucou-assets.mjs` (in `make check` and CI) fails if any `*.xcassets/`, `sounds/`, `docs/media/` or `design/` file lands in the repo.

| Coucou file | Ours |
|---|---|
| `NotchBuddy/Sources/CoucouKit/IslandScreenGeometry.swift` | `crates/macos-panel/src/geometry.rs` |
| `NotchBuddy/Sources/App/IslandDisplayChoice.swift` | `crates/macos-panel/src/display.rs` |
| `NotchBuddy/Sources/App/IslandPlacement.swift` (notch, top corners) | `crates/macos-panel/src/placement.rs` |
| `NotchBuddy/Sources/App/IslandWindowController.swift` (`IslandPanel`, `setupPanel`, `relocate`, `targetScreen`, `displayUUID`, `screenGeometry`) | `crates/macos-panel/src/mac.rs` |
| `IslandWindowController.swift` (`pollFrame`, `adjustPollRate`, screen sleep) + `windows/src-tauri/src/island.rs` (cursor poll, hit margin) | `apps/desktop/src-tauri/src/overlay/poll.rs`, `overlay/mod.rs` |
| `windows/src-tauri/src/island.rs` (monitor keys, work-area offset, `apply_geometry`, `clamp_collapsed`) | `overlay/geometry.rs`, `overlay/mod.rs` |
| `windows/src-tauri/src/platform/{linux,windows}.rs` (layer-shell, Dock, input region, GDK pointer watch, GNOME → XWayland) | `overlay/platform.rs` |
| `NotchBuddy/Sources/App/IslandStateMachine.swift` | `apps/desktop/src/lib/island/fsm.ts` |
| `windows/src/core/anim.ts` | `lib/island/anim.ts` |
| `IslandRootView.swift` (`IslandShape`) | `lib/island/shape.ts` |
| `IslandWindowController.swift` (`islandSize`), `IslandTypes.swift` (`IslandConst`), `windows/src/core/layout.ts` | `lib/island/layout.ts` |
| `ApprovalQueue.swift` + the 115 s wait in `HookServer.swift` | `lib/overlay-model.ts` |
| `IslandRootView.swift` (container, header, mini grid), `IslandViewContent.swift` (approval, empty), `windows/src/views/views.ts`, `windows/src/style.css` | `routes/overlay/+page.svelte` |

**Same as Coucou:** panel 720×560 (macOS) / 720×320 (PC, folding to a 240×6 wake strip while hidden), level `mainMenu+3`, `canJoinAllSpaces | stationary | fullScreenAuxiliary | ignoresCycle`, `constrainFrameRect` returning the frame, non-activating `NSPanel`; notch width = screen − both auxiliary areas (fallback 184), height = top safe-area inset, no-notch bar 80×min(24, menu bar); hidden = the notch (drawn invisible), compact = notch + 160 × notch height, expanded 640×160; radii 14 (rest) / 22 (open) / 18 (corner tile); open spring (0.5, 0.72), close `cubic-bezier(.45,0,.2,1)` 340 ms; hover only peeks (Open on hover off), a click or an approval opens, auto-close 15 s after the pointer leaves (0.6 s if hover-opened), compact hides after 3 min away; click-through poll at 60/20/8 Hz with a 6 pt margin (14 px on a PC), paused while the display sleeps; display choice by UUID (notch / menu bar / follow mouse / display); approvals FIFO, five on the island, 115 s each, cleared when the task ends.

**Our differences (by design):** data from the `task-stream` event and `task-started`, decisions through `approval_respond` and the permission engine; the mascot is a neutral placeholder in Coucou's spot and size (`lib/components/IslandMascot.svelte`, the slot for the owner's own artwork); glyphs are Lucide (house.fill → house, gearshape → settings, xmark → x, chevron.up → chevron-up); no sounds; the overview card is a short "task / Open in Next Agent" card (Coucou's overview shows its agent pills and ticker). On macOS the webview moves from the hidden Tauri window into a real `IslandPanel`, because AppKit observes the Tauri window with KVO, so its class cannot be swapped to `NSPanel`. A request past 115 s or beyond the fifth is not lost: it is still waiting in the app.

**Not verified:** Windows and Linux (the Linux half cannot be compiled on a Mac; layer-shell is behind the opt-in `layer-shell` cargo feature, which needs `libgtk-layer-shell`); multi-monitor moves; display sleep. On this Mac: the panel at layer 27, y = 0, centred; hidden, hover-peek and expanded states in the page; clicks and hover in the real panel could not be driven from the sandbox.
