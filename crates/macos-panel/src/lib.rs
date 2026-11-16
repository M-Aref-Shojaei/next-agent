//! The overlay island's window geometry, ported from Coucou, and its macOS
//! panel.
//!
//! - [`geometry`]: the resting island on a screen (notch size, or the small
//!   no-notch bar).
//! - [`display`]: which screen hosts the island, by stable display id.
//! - [`placement`]: where the panel goes on that screen (notch / top corners)
//!   and where the island sits inside the panel.
//! - `mac` (macOS only): Coucou's `IslandPanel` — a non-activating `NSPanel`
//!   at `mainMenu + 3` that may cover the notch — plus screen enumeration.
//!
//! The first three are pure and compiled and tested on every platform; the
//! Windows/Linux runtime in the desktop app uses them too. This is the only
//! crate allowed `unsafe`, and only in `mac.rs` (each site documented there).
//!
//! Ported from Coucou (MIT, © 2026 Louis Raillé), commit
//! `0d7fe58884d99f275f3238e8dbe29ddee69fa9e8`; see `NOTICE.md`. Code only:
//! no names, characters, icons or sounds.

pub mod display;
pub mod geometry;
pub mod placement;

#[cfg(target_os = "macos")]
mod mac;
/// The few macOS system calls Coucou's macOS layer needs (vendored Coucou,
/// `apps/desktop/coucou/src-tauri/src/platform/macos.rs`), kept here so no
/// other crate needs `unsafe`.
#[cfg(target_os = "macos")]
pub mod sys;

pub use display::{resolve, DisplayCandidate, DisplayChoice};
pub use geometry::{IslandScreenGeometry, FALLBACK_NOTCH_WIDTH};
pub use placement::{
    clamp_collapsed, Placement, Rect, PANEL_H, PANEL_W, PC_PANEL_H, PC_PANEL_W, STRIP_H, STRIP_W,
};

#[cfg(target_os = "macos")]
pub use mac::{
    adopt, cursor_in_panel, display_asleep, notch_at, order_front, place, place_frame, release,
    screens, set_ignores_mouse, set_key, Placed, ScreenEntry, PANEL_LEVEL,
};

/// Window level used by the overlay on macOS: three above
/// `NSMainMenuWindowLevel` (24).
#[cfg(not(target_os = "macos"))]
pub const PANEL_LEVEL: isize = 27;
