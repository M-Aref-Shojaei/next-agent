//! macOS-only window helpers for the overlay.
//!
//! Only public `AppKit` API is used (`NSWindow.level`, `collectionBehavior`,
//! `NSScreen.safeAreaInsets`, `NSScreen.auxiliaryTop*Area`). Nothing here is
//! applied on other platforms; the pure geometry is still compiled and tested
//! everywhere.
//!
//! Design adapted from Coucou (MIT, © 2026 Louis Raillé): `NotchBuddy/` — notch
//! detection from safe-area insets and panel-like window behaviour. No assets.

/// Width used when the auxiliary areas cannot be read (points).
pub const FALLBACK_NOTCH_WIDTH: f64 = 184.0;

/// Size of the physical notch in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notch {
    pub width: f64,
    pub height: f64,
}

/// Derive the notch from `NSScreen` values: a top safe-area inset above zero
/// means there is a notch; its width is what the two auxiliary areas leave.
#[must_use]
pub fn notch_from_insets(
    screen_width: f64,
    top_inset: f64,
    left_aux_width: f64,
    right_aux_width: f64,
) -> Option<Notch> {
    if top_inset <= 0.0 {
        return None;
    }
    let measured = screen_width - left_aux_width - right_aux_width;
    let width = if measured > 0.0 && measured < screen_width / 2.0 {
        measured
    } else {
        FALLBACK_NOTCH_WIDTH
    };
    Some(Notch {
        width,
        height: top_inset,
    })
}

/// Window level used by the overlay: three above `NSMainMenuWindowLevel` (24).
pub const PANEL_LEVEL: isize = 27;

#[cfg(target_os = "macos")]
mod mac {
    use super::{notch_from_insets, Notch, PANEL_LEVEL};
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    use std::ffi::c_void;

    #[allow(unsafe_code)] // Tauri exposes the NSWindow only as a raw pointer.
    fn window<'a>(ns_window: *mut c_void) -> Option<&'a NSWindow> {
        // SAFETY: callers pass the pointer from `WebviewWindow::ns_window()`
        // and use the result on the main thread within the same call, while
        // the window is alive. Null is handled.
        unsafe { ns_window.cast::<NSWindow>().as_ref() }
    }

    /// Make the window behave like a notch panel: above the menu bar, on
    /// every space, alongside full-screen apps, outside Cmd-Tab cycling.
    /// Must run on the main thread. Returns `false` if it could not apply.
    #[must_use]
    pub fn configure_panel(ns_window: *mut c_void) -> bool {
        if MainThreadMarker::new().is_none() {
            return false;
        }
        let Some(w) = window(ns_window) else {
            return false;
        };
        w.setLevel(PANEL_LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        true
    }

    /// Notch of the screen the window is on (`None`: no notch or unknown).
    /// Must run on the main thread.
    #[must_use]
    pub fn notch_of(ns_window: *mut c_void) -> Option<Notch> {
        MainThreadMarker::new()?;
        let screen: Retained<_> = window(ns_window)?.screen()?;
        let width = screen.frame().size.width;
        let inset = screen.safeAreaInsets().top;
        notch_from_insets(
            width,
            inset,
            screen.auxiliaryTopLeftArea().size.width,
            screen.auxiliaryTopRightArea().size.width,
        )
    }
}

#[cfg(target_os = "macos")]
pub use mac::{configure_panel, notch_of};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_inset_means_no_notch() {
        assert_eq!(notch_from_insets(1512.0, 0.0, 700.0, 700.0), None);
    }

    #[test]
    fn notch_width_is_what_the_aux_areas_leave() {
        let n = notch_from_insets(1512.0, 32.0, 664.0, 664.0).expect("notch");
        assert_eq!(n.width, 184.0);
        assert_eq!(n.height, 32.0);
        let n = notch_from_insets(1728.0, 38.0, 770.0, 770.0).expect("notch");
        assert_eq!(n.width, 188.0);
    }

    #[test]
    fn implausible_measurement_falls_back() {
        let n = notch_from_insets(1512.0, 32.0, 0.0, 0.0).expect("notch");
        assert_eq!(n.width, FALLBACK_NOTCH_WIDTH);
    }
}
