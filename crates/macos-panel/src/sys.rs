// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/UserIdentity.swift, TerminalTarget+AppKit.swift, IslandWindowController.swift, FlowDictation.swift, ClaudeHost.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
//! The system calls Coucou's macOS layer needs, as safe functions
//! (`apps/desktop/coucou/src-tauri/src/platform/macos.rs` and
//! `flow/os_macos.rs` call them). Coucou's Windows and Linux layers make the
//! same calls with Win32 / libc; its Mac app makes them from Swift:
//!
//! - `NSFullUserName()` (`UserIdentity.swift`);
//! - `NSEvent.mouseLocation` / `pressedMouseButtons` (`IslandWindowController`);
//! - `NSWorkspace.frontmostApplication`, `runningApplications`,
//!   `NSRunningApplication(processIdentifier:)` and its `bundleURL`
//!   (`TerminalTarget+AppKit.swift`, `FlowDictation.swift`);
//! - `localtime_r` and `getuid` from libc.
//!
//! # Unsafe code in this file
//!
//! 1. `local_time`: `localtime_r` writes into a `tm` we own.
//! 2. `uid`: `getuid` cannot fail and touches no memory.
//! 3. `display_scale_at`: `CGGetDisplaysWithPoint` writes at most one id into
//!    a one-element array we own.

use objc2_app_kit::{NSApplicationActivationPolicy, NSEvent, NSRunningApplication, NSWorkspace};
use objc2_core_graphics::{
    CGDisplayCopyDisplayMode, CGDisplayMode, CGEvent, CGGetDisplaysWithPoint,
};
use objc2_foundation::NSFullUserName;

/// Wall-clock time in the user's time zone: (year, month 1–12, day, hour,
/// minute, second).
#[allow(unsafe_code)] // (1)
#[must_use]
pub fn local_time() -> (u32, u32, u32, u32, u32, u32) {
    // SAFETY: `tm` is plain old data, zero is a valid value for every field,
    // and `localtime_r` only writes into it (reentrant, no static buffer).
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: a null argument asks `time` for the current time only.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: both pointers are valid for the duration of the call.
    unsafe { libc::localtime_r(&raw const now, &raw mut tm) };
    let field = |v: libc::c_int| u32::try_from(v).unwrap_or(0);
    (
        field(tm.tm_year + 1900),
        field(tm.tm_mon + 1),
        field(tm.tm_mday),
        field(tm.tm_hour),
        field(tm.tm_min),
        field(tm.tm_sec),
    )
}

/// The user id this process runs as.
#[allow(unsafe_code)] // (2)
#[must_use]
pub fn uid() -> u32 {
    // SAFETY: `getuid` always succeeds and has no side effects.
    unsafe { libc::getuid() }
}

/// The account's full name ("Louis Raillé"), or nothing when it is unset.
#[must_use]
pub fn full_user_name() -> Option<String> {
    let name = NSFullUserName().to_string();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The pointer in global top-left coordinates, in points (y down from the top
/// of the menu-bar screen). Any thread: it reads a fresh `CGEvent`.
#[must_use]
pub fn cursor_points() -> Option<(f64, f64)> {
    let event = CGEvent::new(None)?;
    let p = CGEvent::location(Some(&event));
    Some((p.x, p.y))
}

/// Backing scale of the display under a global point (2.0 on a Retina
/// screen), 1.0 when none is found. Any thread.
#[allow(unsafe_code)] // (3)
#[must_use]
pub fn display_scale_at(x: f64, y: f64) -> f64 {
    let mut ids = [0u32; 1];
    let mut count = 0u32;
    // SAFETY: room for exactly one id, and `count` is a valid out pointer.
    let err = unsafe {
        CGGetDisplaysWithPoint(
            objc2_core_foundation::CGPoint::new(x, y),
            1,
            ids.as_mut_ptr(),
            &raw mut count,
        )
    };
    if err != objc2_core_graphics::CGError::Success || count == 0 {
        return 1.0;
    }
    let Some(mode) = CGDisplayCopyDisplayMode(ids[0]) else {
        return 1.0;
    };
    let points = CGDisplayMode::width(Some(&mode));
    let pixels = CGDisplayMode::pixel_width(Some(&mode));
    if points == 0 || pixels == 0 {
        return 1.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let scale = pixels as f64 / points as f64;
    scale
}

/// True while the left mouse button is held.
#[must_use]
pub fn left_button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

/// Whether the key with this virtual key code is held now (the combined
/// session state, as FlowShortcut.swift reads modifier flags). Any thread.
#[must_use]
pub fn key_down(code: u16) -> bool {
    objc2_core_graphics::CGEventSource::key_state(
        objc2_core_graphics::CGEventSourceStateID::CombinedSessionState,
        code,
    )
}

/// ⌘V (⇧⌘V with `shift`) to the app in front, posted at the HID level as
/// FlowDictation.swift and SessionDrop.swift do. macOS delivers it only once
/// the user has allowed Accessibility for the app.
#[must_use]
pub fn post_paste(shift: bool) -> bool {
    use objc2_core_graphics::{CGEventFlags, CGEventTapLocation};
    const KEY_V: u16 = 9;
    let flags = if shift {
        CGEventFlags::MaskCommand | CGEventFlags::MaskShift
    } else {
        CGEventFlags::MaskCommand
    };
    for down in [true, false] {
        let Some(event) = CGEvent::new_keyboard_event(None, KEY_V, down) else {
            return false;
        };
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    true
}

fn app_entry(app: &NSRunningApplication) -> Option<(String, String)> {
    let id = app.bundleIdentifier()?.to_string();
    let name = app
        .localizedName()
        .map_or_else(|| id.clone(), |n| n.to_string());
    Some((id, name))
}

/// The app in front: (bundle id, name).
#[must_use]
pub fn frontmost_app() -> Option<(String, String)> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app_entry(&app)
}

/// Apps with a Dock icon, as (bundle id, name).
#[must_use]
pub fn running_apps() -> Vec<(String, String)> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
        .filter_map(|a| app_entry(&a))
        .collect()
}

/// The `.app` bundle of a running process that has one (a GUI app), as a path.
#[must_use]
pub fn app_bundle_path(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
        return None;
    }
    let url = app.bundleURL()?;
    let path = url.path()?.to_string();
    std::path::Path::new(&path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        .then_some(path)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_a_plausible_date() {
        let (year, month, day, hour, minute, second) = local_time();
        assert!(year >= 2024);
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
        assert!(hour < 24 && minute < 60 && second < 61);
    }

    #[test]
    fn the_user_id_matches_the_home_folder_owner() {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::var_os("HOME").expect("HOME");
        let owner = std::fs::metadata(home).expect("home").uid();
        assert_eq!(uid(), owner);
    }
}
