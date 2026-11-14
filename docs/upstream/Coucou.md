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

## Implementation status (Wave 1, overlay)
- Rust: `apps/desktop/src-tauri/src/overlay.rs` (placement, monitor choice, click-through state machine, `task-stream` fan-out, window + commands); `crates/macos-panel` (notch detection, window level, collection behaviour via objc2; the only crate allowed `unsafe`, for one pointer deref).
- Frontend: route `/overlay`, `lib/overlay-model.ts` (FIFO queue, max 5 visible), settings section in `/settings`.
- Task events reach the overlay through the Tauri event `task-stream` (events only) emitted next to the per-task Channel; `task-started` carries title and mode. Approvals are answered through the existing `approval_respond` command.
- macOS: the webview is **opaque** (a transparent webview needs Tauri's private `macos-private-api` flag, which we do not enable). The island is a dark rectangle that blends with the notch. A true NSPanel (`.nonactivatingPanel`) needs an NSWindow subclass swap and was skipped; we use `focusable(false)` plus level `mainMenu+3`.
- Linux input region / layer-shell: skipped. The cursor poll toggles `set_ignore_cursor_events`, which works on X11 and most XWayland setups; native Wayland compositors may ignore it.
- Not verified without a display: everything in the window runtime (placement on real monitors, click-through, notch panel behaviour, animations).
