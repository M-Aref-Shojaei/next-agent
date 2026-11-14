// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/platform/linux.rs and windows/src-tauri/src/platform/windows.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// What differs between Windows and Linux for the overlay window.
//
// Windows: Tauri already gives what Coucou's Win32 calls do — `focusable(false)`
// (no activation), `skip_taskbar` (tool window), `set_ignore_cursor_events`
// (WS_EX_TRANSPARENT click-through) — so nothing here needs `unsafe`.
//
// Linux: Wayland gives an app no global cursor position and no say over where
// its window goes, so, as in Coucou:
//   * the overlay is a layer-shell surface anchored to the top edge, above
//     everything, where the compositor supports it (wlroots, KDE, COSMIC) and
//     the `layer-shell` feature is built in;
//   * otherwise it is a Dock window (kept above, survives "show desktop"),
//     and on GNOME Wayland the app runs through XWayland so it can be placed;
//   * click-through is the window's input region, set to the island;
//   * a 100 ms GDK pointer watch reports the pointer entering and leaving.
// The Linux half cannot be compiled on a Mac; it follows Coucou's calls on the
// same GTK 0.18 bindings Tauri uses.

/// Whether the cursor poll drives click-through (Windows, macOS). Linux uses
/// the input region and the pointer watch instead.
pub const CURSOR_POLL: bool = !cfg!(target_os = "linux");

/// GNOME on Wayland has no layer-shell and ignores where a regular window asks
/// to go; through `XWayland` it can be placed. Only with `XWayland` actually
/// there (`DISPLAY` set). `NEXT_AGENT_X11=0` keeps the Wayland window.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[must_use]
pub fn should_prefer_x11(session: &str, desktop: &str, opt_out: &str, display: &str) -> bool {
    session.eq_ignore_ascii_case("wayland")
        && desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome"))
        && opt_out != "0"
        && !display.trim().is_empty()
}

/// Run before Tauri starts (GTK reads `GDK_BACKEND` at init).
pub fn prepare_environment() {
    #[cfg(target_os = "linux")]
    {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        if should_prefer_x11(
            &env("XDG_SESSION_TYPE"),
            &env("XDG_CURRENT_DESKTOP"),
            &env("NEXT_AGENT_X11"),
            &env("DISPLAY"),
        ) {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{make_overlay_window, pin_to_monitor, set_input_region};

#[cfg(not(target_os = "linux"))]
mod other {
    use next_agent_macos_panel::Placement;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;
    use tauri::WebviewWindow;

    pub fn make_overlay_window(
        _win: &WebviewWindow,
        _placement: Placement,
        _alive: Arc<AtomicU64>,
        _generation: u64,
    ) {
    }
    pub fn pin_to_monitor(_win: &WebviewWindow, _x: i32, _y: i32, _placement: Placement) {}
    pub fn set_input_region(_win: &WebviewWindow, _rect: Option<(f64, f64, f64, f64)>) {}
}

#[cfg(not(target_os = "linux"))]
pub use other::{make_overlay_window, pin_to_monitor, set_input_region};

#[cfg(target_os = "linux")]
mod linux {
    use gtk::prelude::*;
    use next_agent_macos_panel::Placement;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use tauri::{Emitter, WebviewWindow};

    /// True once the overlay is a layer-shell surface.
    static LAYER_SURFACE: AtomicBool = AtomicBool::new(false);

    /// The input region last asked for, re-applied whenever the window is
    /// mapped (GTK resets it to the whole window on map). Until the page
    /// reports the island it is empty, so nothing takes the mouse.
    type Region = Option<(f64, f64, f64, f64)>;
    static INPUT_REGION: Mutex<Region> = Mutex::new(Some((0.0, 0.0, 0.0, 0.0)));

    fn region() -> Region {
        *INPUT_REGION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Turn the overlay into an overlay surface on the top edge that never
    /// takes the keyboard. Must run before the window is first shown. Without
    /// layer-shell it stays a regular always-on-top window that refuses focus
    /// (a Dock on X11; `NEXT_AGENT_DOCK=0` makes it a utility window).
    /// Also starts the pointer watch, which stops when `alive` moves past
    /// `generation`. GTK main thread only.
    pub fn make_overlay_window(
        win: &WebviewWindow,
        placement: Placement,
        alive: Arc<AtomicU64>,
        generation: u64,
    ) {
        let Ok(gw) = win.gtk_window() else { return };
        watch_pointer(win, &gw, alive, generation);
        if try_layer_shell(&gw, placement) {
            LAYER_SURFACE.store(true, Ordering::Relaxed);
        } else {
            gw.set_accept_focus(false);
            if !gw.is_realized() {
                let dock = std::env::var("NEXT_AGENT_DOCK").map_or(true, |v| v != "0");
                gw.set_type_hint(if dock {
                    gtk::gdk::WindowTypeHint::Dock
                } else {
                    gtk::gdk::WindowTypeHint::Utility
                });
            }
            gw.set_keep_above(true);
        }
        gw.connect_map_event(|w, _| {
            apply_input_region(w, region());
            gtk::glib::Propagation::Proceed
        });
    }

    #[cfg(feature = "layer-shell")]
    fn try_layer_shell(gw: &gtk::ApplicationWindow, placement: Placement) -> bool {
        use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
        let wanted = std::env::var("NEXT_AGENT_LAYER_SHELL").map_or(true, |v| v != "0");
        if !wanted || !gtk_layer_shell::is_supported() || gw.is_realized() {
            return false;
        }
        // A client-decorated GtkWindow recomputes its own input region on
        // every map, over ours; a layer surface has no titlebar.
        gw.set_titlebar(None::<&gtk::Widget>);
        gw.init_layer_shell();
        LayerShell::set_namespace(gw, "next-agent-overlay");
        LayerShell::set_layer(gw, Layer::Overlay);
        // Top edge only: the compositor centres the surface horizontally.
        gw.set_anchor(Edge::Top, true);
        gw.set_anchor(Edge::Left, placement == Placement::TopLeft);
        gw.set_anchor(Edge::Right, placement == Placement::TopRight);
        // -1: right against the screen edge, over any top panel.
        gw.set_exclusive_zone(-1);
        gw.set_keyboard_mode(KeyboardMode::None);
        // WebKitGTK in a freshly mapped layer surface never paints its first
        // frame; unmapping and mapping it once right after gets it drawing.
        let remapped = std::cell::Cell::new(false);
        gw.connect_map_event(move |w, _| {
            if !remapped.replace(true) {
                let w = w.clone();
                gtk::glib::idle_add_local_once(move || {
                    w.hide();
                    w.show_all();
                    apply_input_region(&w, region());
                });
            }
            gtk::glib::Propagation::Proceed
        });
        true
    }

    #[cfg(not(feature = "layer-shell"))]
    fn try_layer_shell(_gw: &gtk::ApplicationWindow, _placement: Placement) -> bool {
        false
    }

    /// Put the layer surface on the display whose logical origin is (`x`,
    /// `y`): a layer surface ignores `set_position`, and without an explicit
    /// output the compositor maps it on whichever display has focus. A no-op
    /// for a regular window, which `set_position` already places.
    pub fn pin_to_monitor(win: &WebviewWindow, x: i32, y: i32, placement: Placement) {
        if !LAYER_SURFACE.load(Ordering::Relaxed) {
            return;
        }
        let Ok(gw) = win.gtk_window() else { return };
        layer_pin(&gw, x, y, placement);
    }

    #[cfg(feature = "layer-shell")]
    fn layer_pin(gw: &gtk::ApplicationWindow, x: i32, y: i32, placement: Placement) {
        use gtk_layer_shell::{Edge, LayerShell};
        gw.set_anchor(Edge::Left, placement == Placement::TopLeft);
        gw.set_anchor(Edge::Right, placement == Placement::TopRight);
        let display = gw.display();
        let monitor = (0..display.n_monitors())
            .filter_map(|i| display.monitor(i))
            .find(|m| {
                let g = m.geometry();
                g.x() == x && g.y() == y
            });
        if let Some(monitor) = monitor {
            LayerShell::set_monitor(gw, &monitor);
        }
    }

    #[cfg(not(feature = "layer-shell"))]
    fn layer_pin(_gw: &gtk::ApplicationWindow, _x: i32, _y: i32, _placement: Placement) {}

    /// Only this rectangle (window-logical px) takes the mouse; `None` means
    /// the whole window does. Everything outside goes to what is underneath.
    pub fn set_input_region(win: &WebviewWindow, rect: Region) {
        *INPUT_REGION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = rect;
        let Ok(gw) = win.gtk_window() else { return };
        apply_input_region(&gw, rect);
    }

    #[allow(clippy::cast_possible_truncation)]
    fn apply_input_region(gw: &impl IsA<gtk::Widget>, rect: Region) {
        match rect {
            None => gw.input_shape_combine_region(None),
            Some((x, y, w, h)) => {
                let Some(gdk_window) = gw.window() else {
                    return;
                };
                let region = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                    x.floor() as i32,
                    y.floor() as i32,
                    w.ceil().max(0.0) as i32,
                    h.ceil().max(0.0) as i32,
                ));
                gdk_window.input_shape_combine_region(&region, 0, 0);
            }
        }
    }

    /// Reports the pointer entering and leaving the overlay as
    /// `overlay-pointer`. On a layer surface WebKitGTK sends no `mouseout`
    /// when the pointer leaves for good; GDK's pointer focus follows the
    /// compositor's own enter/leave.
    fn watch_pointer(
        win: &WebviewWindow,
        gw: &gtk::ApplicationWindow,
        alive: Arc<AtomicU64>,
        generation: u64,
    ) {
        let win = win.clone();
        let gw = gw.clone();
        let inside = std::cell::Cell::new(false);
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if alive.load(Ordering::SeqCst) != generation {
                return gtk::glib::ControlFlow::Break;
            }
            let now = (|| {
                let ours = gw.window()?;
                let pointer = gtk::gdk::Display::default()?.default_seat()?.pointer()?;
                let (under, _, _) = pointer.window_at_position();
                Some(under?.toplevel() == ours.toplevel())
            })()
            .unwrap_or(false);
            if inside.replace(now) != now {
                let _ = win.emit("overlay-pointer", now);
            }
            gtk::glib::ControlFlow::Continue
        });
    }
}

#[cfg(test)]
mod tests {
    use super::should_prefer_x11;

    #[test]
    fn gnome_wayland_with_xwayland_prefers_x11_unless_opted_out() {
        assert!(should_prefer_x11("wayland", "ubuntu:GNOME", "", ":0"));
        assert!(!should_prefer_x11("wayland", "ubuntu:GNOME", "0", ":0"));
        assert!(
            !should_prefer_x11("wayland", "GNOME", "", ""),
            "no XWayland"
        );
        assert!(!should_prefer_x11("wayland", "KDE", "", ":0"));
        assert!(!should_prefer_x11("x11", "GNOME", "", ":0"));
    }
}
