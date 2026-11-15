// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandWindowController.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// The AppKit side of the island: Coucou's `IslandPanel` (an `NSPanel` that
// may sit in the menu bar / notch), created like Coucou creates it
// (`styleMask: [.borderless, .nonactivatingPanel]`) and configured like its
// `setupPanel`, the click-through toggle, the screen choice by display UUID,
// and `relocate(to:)`.
//
// Coucou puts a SwiftUI hosting view in its panel; we put the Tauri webview
// there: `adopt` moves the content view of the (hidden) Tauri window into the
// panel. The Tauri window itself cannot become an `NSPanel` safely: AppKit
// observes it with KVO, so swapping its class would break the observers.
//
// # Unsafe code in this file
//
// Everything runs on the main thread (checked with `MainThreadMarker`) and
// uses public AppKit / CoreGraphics / ColorSync API only. `unsafe` sites,
// each with its own `SAFETY` note:
//
// 1. `window`: the `*mut c_void` Tauri hands out (`WebviewWindow::ns_window`)
//    turned into `&NSWindow`.
// 2. `define_class!`: the `IslandPanel` subclass of `NSPanel` (no ivars;
//    three overrides with AppKit's exact signatures).
// 3. `new_panel`: `initWithContentRect:styleMask:backing:defer:` sent to the
//    subclass, and `setReleasedWhenClosed:`.
// 4. `display_uuid`: one ColorSync C call that follows the Create rule.

use crate::display::{resolve, DisplayCandidate, DisplayChoice};
use crate::geometry::{menu_bar_height, IslandScreenGeometry};
use crate::placement::{Placement, Rect};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSScreen, NSStatusBar, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CFUUID};
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

/// Three above `NSMainMenuWindowLevel` (24): over the menu bar and the notch
/// (`CGWindowLevelForKey(.mainMenuWindow) + 3` in Coucou).
pub const PANEL_LEVEL: isize = 27;

#[allow(unsafe_code)] // (1) Tauri exposes the NSWindow only as a raw pointer.
fn window<'a>(ns_window: *mut c_void) -> Option<&'a NSWindow> {
    // SAFETY: callers pass the pointer from `WebviewWindow::ns_window()` and
    // use the result on the main thread within the same call, while Tauri
    // keeps the window alive. Null is handled by `as_ref`.
    unsafe { ns_window.cast::<NSWindow>().as_ref() }
}

#[allow(unsafe_code)] // (2) Declaring an Objective-C subclass.
mod panel_class {
    use objc2::{define_class, MainThreadOnly};
    use objc2_app_kit::{NSPanel, NSResponder, NSScreen, NSWindow};
    use objc2_foundation::{NSObject, NSRect};

    define_class!(
        // SAFETY: `NSPanel` may be subclassed and has no special subclassing
        // requirements. The class adds no instance variables and no `Drop`.
        // Each override below has exactly the signature AppKit declares.
        #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "NextAgentIslandPanel"]
        pub struct IslandPanel;

        impl IslandPanel {
            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key_window(&self) -> bool {
                true
            }

            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main_window(&self) -> bool {
                false
            }

            /// Allow panel to sit in the menu bar / notch area — don't let
            /// macOS push it down.
            #[unsafe(method(constrainFrameRect:toScreen:))]
            fn constrain_frame_rect(&self, frame: NSRect, _screen: Option<&NSScreen>) -> NSRect {
                frame
            }
        }
    );
}

use panel_class::IslandPanel;

thread_local! {
    /// The island panel (main thread only, like every AppKit window).
    static PANEL: RefCell<Option<Retained<IslandPanel>>> = const { RefCell::new(None) };
}

/// The panel's top-left corner in global top-left coordinates (points, y
/// down from the top of the menu-bar screen), readable from the poll thread.
static ORIGIN: Mutex<Option<(f64, f64)>> = Mutex::new(None);

fn set_origin_cache(origin: Option<(f64, f64)>) {
    *ORIGIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = origin;
}

fn with_panel<R>(f: impl FnOnce(&IslandPanel) -> R) -> Option<R> {
    MainThreadMarker::new()?;
    PANEL.with(|cell| cell.borrow().as_deref().map(f))
}

#[allow(unsafe_code)] // (3) Creating the panel.
fn new_panel(mtm: MainThreadMarker, size: (f64, f64)) -> Retained<IslandPanel> {
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.0, size.1));
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let this = objc2::MainThreadOnly::alloc(mtm);
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is NSWindow's
    // designated initialiser, inherited unchanged by `IslandPanel`; the
    // argument types match its declaration and it returns +1 (init family).
    let panel: Retained<IslandPanel> = unsafe {
        objc2::msg_send![
            this,
            initWithContentRect: rect,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    // SAFETY: we own the panel through `Retained`; AppKit must not release
    // it a second time when it is closed.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel
}

/// `setupPanel`: clear, no shadow, three levels above the menu bar, on every
/// Space and beside full-screen apps, out of window cycling, click-through
/// until the poll says the pointer is on the island.
fn configure(panel: &IslandPanel) {
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setOpaque(false);
    panel.setHasShadow(false);
    panel.setLevel(PANEL_LEVEL);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setIgnoresMouseEvents(true);
    // An NSPanel hides itself when its app deactivates unless told otherwise.
    panel.setHidesOnDeactivate(false);
    panel.setFloatingPanel(false);
    // The panel only becomes key when a view needs the keyboard (Coucou makes
    // it key itself for its text fields; we have none).
    panel.setBecomesKeyOnlyIfNeeded(true);
}

/// Create Coucou's island panel of `size` and move the Tauri window's content
/// (the webview) into it; the Tauri window stays hidden. Main thread only.
///
/// # Errors
/// Why it could not (then the Tauri window keeps its content).
pub fn adopt(ns_window: *mut c_void, size: (f64, f64)) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let host = window(ns_window).ok_or("no window")?;
    if PANEL.with(|cell| cell.borrow().is_some()) {
        return Err("the island panel already exists".into());
    }
    let content = host
        .contentView()
        .ok_or("the Tauri window has no content view")?;
    let panel = new_panel(mtm, size);
    configure(&panel);
    host.orderOut(None);
    panel.setContentView(Some(&content));
    PANEL.with(|cell| *cell.borrow_mut() = Some(panel));
    Ok(())
}

/// Close and drop the panel (the overlay was turned off). Main thread only.
pub fn release() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    if let Some(panel) = PANEL.with(|cell| cell.borrow_mut().take()) {
        panel.orderOut(None);
        panel.setContentView(None);
    }
    set_origin_cache(None);
}

/// Show the panel without making it key. Main thread only.
#[must_use]
pub fn order_front() -> bool {
    with_panel(|p| p.orderFrontRegardless()).is_some()
}

/// Click-through on or off (`panel.ignoresMouseEvents`). Main thread only.
#[must_use]
pub fn set_ignores_mouse(ignore: bool) -> bool {
    with_panel(|p| {
        if p.ignoresMouseEvents() != ignore {
            p.setIgnoresMouseEvents(ignore);
        }
    })
    .is_some()
}

/// The pointer in the panel's own coordinates (y down from its top-left).
/// Reads the location of a fresh `CGEvent`, which is thread-safe (Coucou reads
/// `NSEvent.mouseLocation` on its main-run-loop timer). Any thread.
#[must_use]
pub fn cursor_in_panel() -> Option<(f64, f64)> {
    let (ox, oy) = (*ORIGIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))?;
    let event = objc2_core_graphics::CGEvent::new(None)?;
    let p = objc2_core_graphics::CGEvent::location(Some(&event));
    Some((p.x - ox, p.y - oy))
}

#[link(name = "ColorSync", kind = "framework")]
extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> Option<NonNull<CFUUID>>;
}

/// Stable display UUID (the `NSScreenNumber` can change after a reboot or a
/// replug).
#[allow(unsafe_code)] // (4) ColorSync C call.
fn display_uuid(screen: &NSScreen) -> Option<String> {
    let key = NSString::from_str("NSScreenNumber");
    let number = screen.deviceDescription().objectForKey(&key)?;
    let number = number.downcast::<NSNumber>().ok()?;
    let id = number.unsignedIntValue();
    // SAFETY: a plain C function taking a display id by value; it returns a
    // +1 retained CFUUID (Create rule) or NULL, which `CFRetained::from_raw`
    // takes ownership of exactly once.
    let uuid = unsafe { CGDisplayCreateUUIDFromDisplayID(id).map(|p| CFRetained::from_raw(p)) }?;
    CFUUID::new_string(None, Some(&uuid)).map(|s| s.to_string())
}

fn rect(r: NSRect) -> Rect {
    Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
}

fn contains(r: NSRect, p: NSPoint) -> bool {
    p.x >= r.origin.x
        && p.x < r.origin.x + r.size.width
        && p.y >= r.origin.y
        && p.y < r.origin.y + r.size.height
}

/// `screenGeometry(for:)`.
fn screen_geometry(screen: &NSScreen) -> IslandScreenGeometry {
    let frame = screen.frame();
    let visible = screen.visibleFrame();
    IslandScreenGeometry::new(
        frame.size.width,
        screen.safeAreaInsets().top,
        Some(screen.auxiliaryTopLeftArea().size.width),
        Some(screen.auxiliaryTopRightArea().size.width),
        menu_bar_height(
            frame.origin.y + frame.size.height,
            visible.origin.y + visible.size.height,
            NSStatusBar::systemStatusBar().thickness(),
        ),
    )
}

/// One connected screen, for the settings list.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenEntry {
    pub id: Option<String>,
    pub name: String,
    pub has_notch: bool,
}

/// Connected screens in `NSScreen.screens` order. Main thread only.
#[must_use]
pub fn screens() -> Vec<ScreenEntry> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    NSScreen::screens(mtm)
        .iter()
        .map(|s| ScreenEntry {
            id: display_uuid(&s),
            name: s.localizedName().to_string(),
            has_notch: s.safeAreaInsets().top > 0.0,
        })
        .collect()
}

/// Where the panel went.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The resting island on that screen (notch, or the no-notch bar).
    pub geometry: IslandScreenGeometry,
    pub screen_id: Option<String>,
    /// Screen frame and panel frame, `AppKit` coordinates (y up).
    pub screen: Rect,
    pub frame: Rect,
}

/// `targetScreen(for:)`: the resolver's pick, else the notch screen, else the
/// main screen, else the first.
fn target_screen(choice: &DisplayChoice, mtm: MainThreadMarker) -> Option<Retained<NSScreen>> {
    let list: Vec<Retained<NSScreen>> = NSScreen::screens(mtm).iter().collect();
    let mouse = NSEvent::mouseLocation();
    let candidates: Vec<DisplayCandidate> = list
        .iter()
        .map(|s| DisplayCandidate {
            id: display_uuid(s),
            has_notch: s.safeAreaInsets().top > 0.0,
            contains_mouse: contains(s.frame(), mouse),
        })
        .collect();
    if let Some(i) = resolve(choice, &candidates) {
        return list.get(i).cloned();
    }
    NSScreen::mainScreen(mtm).or_else(|| list.first().cloned())
}

/// `relocate(to:)`: recompute the resting geometry for the target screen and
/// move the panel to its top centre, or to the corner the island lives in.
/// Always re-applied, even on the same screen: its menu bar or resolution may
/// have changed. Main thread only.
#[must_use]
pub fn place(choice: &DisplayChoice, placement: Placement, panel: (f64, f64)) -> Option<Placed> {
    let mtm = MainThreadMarker::new()?;
    let screen = target_screen(choice, mtm)?;
    // `restingGeometry(for:)`: in a corner, the small bar of a screen without a notch.
    let geometry = if placement.is_corner() {
        IslandScreenGeometry::new(
            screen.frame().size.width,
            0.0,
            None,
            None,
            NSStatusBar::systemStatusBar().thickness(),
        )
    } else {
        screen_geometry(&screen)
    };
    let screen_rect = rect(screen.frame());
    let (x, y) = placement.panel_origin(screen_rect, rect(screen.visibleFrame()), panel);
    let frame = Rect::new(x, y, panel.0, panel.1);
    with_panel(|p| {
        p.setLevel(PANEL_LEVEL);
        p.setFrame_display(
            NSRect::new(NSPoint::new(x, y), NSSize::new(panel.0, panel.1)),
            true,
        );
    })?;
    // AppKit's y axis points up from the bottom of the menu-bar screen.
    let top = NSScreen::screens(mtm)
        .iter()
        .next()
        .map_or(0.0, |s| s.frame().size.height);
    set_origin_cache(Some((x, top - (y + panel.1))));
    Some(Placed {
        geometry,
        screen_id: display_uuid(&screen),
        screen: screen_rect,
        frame,
    })
}

/// Put the panel at `(x, y_top)` (global top-left coordinates, points, y down
/// from the top of the menu-bar screen) with size `(w, h)`, above the menu bar.
/// For callers that pick the screen themselves (Coucou's `island.rs` does,
/// from Tauri's monitors). Main thread only.
#[must_use]
pub fn place_frame(x: f64, y_top: f64, w: f64, h: f64) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    // AppKit's y axis points up from the bottom of the menu-bar screen.
    let top = NSScreen::screens(mtm)
        .iter()
        .next()
        .map_or(0.0, |s| s.frame().size.height);
    let y = top - (y_top + h);
    let placed = with_panel(|p| {
        p.setLevel(PANEL_LEVEL);
        p.setFrame_display(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), true);
    })
    .is_some();
    if placed {
        set_origin_cache(Some((x, y_top)));
    }
    placed
}

/// Let the panel take the keyboard (a text field in the island has focus) or
/// give it back. Coucou's Mac app makes its panel key for its text fields.
/// Main thread only.
#[must_use]
pub fn set_key(key: bool) -> bool {
    with_panel(|p| {
        if key {
            p.makeKeyAndOrderFront(None);
        } else if p.isKeyWindow() {
            p.resignKeyWindow();
        }
    })
    .is_some()
}

/// The main display is asleep: nothing to hover, the cursor poll pauses.
/// Any thread.
#[must_use]
pub fn display_asleep() -> bool {
    objc2_core_graphics::CGDisplayIsAsleep(objc2_core_graphics::CGMainDisplayID())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;
    use objc2_app_kit::NSPanel;

    #[test]
    fn island_panel_is_an_nspanel_without_ivars() {
        let panel = IslandPanel::class();
        assert_eq!(panel.superclass(), Some(NSPanel::class()));
        assert_eq!(panel.instance_size(), NSWindow::class().instance_size());
    }
}
