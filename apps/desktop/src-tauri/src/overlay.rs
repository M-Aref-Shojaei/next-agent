//! Always-on-top overlay: a second, click-through Tauri window that shows the
//! active task and pending approvals.
//!
//! Design adapted from Coucou (MIT, © 2026 Louis Raillé):
//! `windows/src-tauri/src/island.rs` and `tauri.conf.json` — window flags,
//! the size → position → size → always-on-top sequence, monitor choice and the
//! cursor-poll click-through. No assets are copied.
//!
//! The pure parts (`place`, `pick_monitor`, [`ClickThrough`], [`fanout`]) are
//! unit-tested; the window plumbing below them needs a display.

use crate::service::{AppService, Sink};
use crate::wire::StreamMessage;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "overlay";
const SETTINGS_KEY: &str = "overlay.settings";
/// Expanded panel size in logical pixels.
pub const EXPANDED: (f64, f64) = (720.0, 320.0);
/// Collapsed pill size in logical pixels (no notch).
pub const COLLAPSED: (f64, f64) = (240.0, 28.0);
/// Gap kept to a screen corner for the corner positions (logical px).
const CORNER_MARGIN: f64 = 16.0;
/// Extra hit area around the island (logical px).
pub const MARGIN: f64 = 14.0;
/// Distance from the island that still counts as "near" (logical px).
const NEAR: f64 = 120.0;
pub const POLL_NEAR: Duration = Duration::from_millis(16);
pub const POLL_FAR: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------- settings --

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    #[default]
    TopCenter,
    TopLeft,
    TopRight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub position: OverlayPosition,
    /// Stable monitor name; `None` follows the cursor, then the primary.
    pub monitor: Option<String>,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            position: OverlayPosition::default(),
            monitor: None,
        }
    }
}

#[must_use]
pub fn load_settings(service: &AppService) -> OverlaySettings {
    service
        .store()
        .ok()
        .and_then(|s| s.get_setting(SETTINGS_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// # Errors
/// Database or serialisation errors.
pub fn save_settings(service: &AppService, settings: &OverlaySettings) -> Result<(), String> {
    let raw = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    service
        .store()?
        .set_setting(SETTINGS_KEY, &raw)
        .map_err(|e| e.to_string())
}

// --------------------------------------------------------------- placement --

/// One display in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorGeom {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub work_x: i32,
    pub work_y: i32,
    pub work_w: u32,
    pub work_h: u32,
    pub scale: f64,
}

/// Window rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn px(logical: f64, scale: f64) -> u32 {
    (logical * scale).round().max(1.0) as u32
}

#[allow(clippy::cast_possible_truncation)]
fn pxi(logical: f64, scale: f64) -> i32 {
    (logical * scale).round() as i32
}

/// Collapsed pill size: wider and as tall as the notch when there is one.
#[must_use]
pub fn collapsed_size(notch: Option<(f64, f64)>) -> (f64, f64) {
    match notch {
        Some((w, h)) => ((w + 140.0).max(COLLAPSED.0), h.max(COLLAPSED.1)),
        None => COLLAPSED,
    }
}

/// Where the window goes on `monitor`. With `use_work_area` (Windows) the top
/// edge is the work-area top so a top taskbar is not covered; otherwise the
/// physical screen top (macOS: over the menu bar / notch).
#[must_use]
pub fn place(
    monitor: &MonitorGeom,
    logical: (f64, f64),
    position: OverlayPosition,
    use_work_area: bool,
) -> Placed {
    let w = px(logical.0, monitor.scale).min(monitor.w);
    let h = px(logical.1, monitor.scale);
    let (left, top, width) = if use_work_area {
        (monitor.work_x, monitor.work_y, monitor.work_w)
    } else {
        (monitor.x, monitor.y, monitor.w)
    };
    let free = i64::from(width) - i64::from(w);
    let margin = i64::from(pxi(CORNER_MARGIN, monitor.scale));
    let offset = match position {
        OverlayPosition::TopCenter => free / 2,
        OverlayPosition::TopLeft => margin.min(free),
        OverlayPosition::TopRight => (free - margin).max(0),
    };
    #[allow(clippy::cast_possible_truncation)]
    let x = (i64::from(left) + offset) as i32;
    Placed { x, y: top, w, h }
}

/// Monitor choice: saved name, else the one under the cursor, else the
/// primary (by name), else the first.
#[must_use]
pub fn pick_monitor(
    monitors: &[MonitorGeom],
    saved: Option<&str>,
    cursor: Option<(f64, f64)>,
    primary: Option<&str>,
) -> Option<usize> {
    if let Some(name) = saved {
        if let Some(i) = monitors
            .iter()
            .position(|m| m.name.as_deref() == Some(name))
        {
            return Some(i);
        }
    }
    if let Some((cx, cy)) = cursor {
        let hit = monitors.iter().position(|m| {
            cx >= f64::from(m.x)
                && cx < f64::from(m.x) + f64::from(m.w)
                && cy >= f64::from(m.y)
                && cy < f64::from(m.y) + f64::from(m.h)
        });
        if hit.is_some() {
            return hit;
        }
    }
    if let Some(name) = primary {
        if let Some(i) = monitors
            .iter()
            .position(|m| m.name.as_deref() == Some(name))
        {
            return Some(i);
        }
    }
    (!monitors.is_empty()).then_some(0)
}

// ------------------------------------------------------------ click-through --

/// Island rectangle in CSS pixels relative to the window's top-left, supplied
/// by the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn grown(self, by: f64) -> Self {
        Self {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2.0 * by,
            h: self.h + 2.0 * by,
        }
    }
    fn contains(self, (px, py): (f64, f64)) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Result of one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// `Some(inside)` only when the pointer entered/left the island's hit
    /// area. The window ignores cursor events exactly while not inside.
    pub changed: Option<bool>,
    pub next_poll: Duration,
}

/// Click-through state machine: toggles only on change, polls fast near the
/// island and slowly far away.
#[derive(Debug, Default)]
pub struct ClickThrough {
    inside: Option<bool>,
}

impl ClickThrough {
    /// `cursor` is physical pixels relative to the window's top-left (`None`
    /// when unknown); `rect` is in CSS px; `scale` maps CSS to physical px.
    pub fn tick(&mut self, cursor: Option<(f64, f64)>, rect: Option<Rect>, scale: f64) -> Tick {
        let scaled = rect.map(|r| Rect {
            x: r.x * scale,
            y: r.y * scale,
            w: r.w * scale,
            h: r.h * scale,
        });
        let inside = match (cursor, scaled) {
            (Some(c), Some(r)) => r.grown(MARGIN * scale).contains(c),
            _ => false,
        };
        let near = match (cursor, scaled) {
            (Some(c), Some(r)) => r.grown((MARGIN + NEAR) * scale).contains(c),
            _ => false,
        };
        let changed = (self.inside != Some(inside)).then_some(inside);
        self.inside = Some(inside);
        Tick {
            changed,
            next_poll: if near { POLL_NEAR } else { POLL_FAR },
        }
    }
}

// ------------------------------------------------------------- task stream --

/// Wrap a task sink so every sequenced event is also broadcast. Only events
/// are broadcast (not review diffs), which is all the overlay needs.
#[must_use]
pub fn fanout(direct: Sink, broadcast: Sink) -> Sink {
    Arc::new(move |message| {
        if matches!(message, StreamMessage::Event { .. }) {
            broadcast(message.clone());
        }
        direct(message);
    })
}

// ------------------------------------------------------------ window runtime --

#[derive(Debug, Default)]
struct Inner {
    rect: Option<Rect>,
    expanded: bool,
    /// macOS notch (width, height) in points.
    notch: Option<(f64, f64)>,
}

/// Managed overlay state.
#[derive(Debug, Default)]
pub struct OverlayState {
    inner: Mutex<Inner>,
    /// Bumped on every window (re)creation so stale pollers exit.
    generation: AtomicU64,
}

pub type OverlayHandle = Arc<OverlayState>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    pub settings: OverlaySettings,
    pub monitors: Vec<String>,
    /// `false` on macOS: a transparent webview needs Tauri's private-API
    /// flag, which we do not enable, so the page paints an opaque island.
    pub transparent: bool,
    pub notch: Option<(f64, f64)>,
}

fn geoms(app: &AppHandle) -> Vec<MonitorGeom> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let wa = m.work_area();
            MonitorGeom {
                name: m.name().cloned(),
                x: m.position().x,
                y: m.position().y,
                w: m.size().width,
                h: m.size().height,
                work_x: wa.position.x,
                work_y: wa.position.y,
                work_w: wa.size.width,
                work_h: wa.size.height,
                scale: m.scale_factor(),
            }
        })
        .collect()
}

/// Size → position → size → always-on-top, as Coucou does (moving between
/// monitors can rescale the window; Linux needs resizable to shrink).
fn apply(app: &AppHandle, state: &OverlayState, service: &AppService) -> Result<(), String> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "overlay window is not open".to_string())?;
    let settings = load_settings(service);
    let monitors = geoms(app);
    let cursor = app.cursor_position().ok().map(|p| (p.x, p.y));
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());
    let index = pick_monitor(
        &monitors,
        settings.monitor.as_deref(),
        cursor,
        primary.as_deref(),
    )
    .ok_or_else(|| "no monitor found".to_string())?;
    let (expanded, notch) = {
        let inner = state.inner.lock().map_err(|e| e.to_string())?;
        (inner.expanded, inner.notch)
    };
    let logical = if expanded {
        EXPANDED
    } else {
        collapsed_size(notch)
    };
    let placed = place(&monitors[index], logical, settings.position, cfg!(windows));
    let size = PhysicalSize::new(placed.w, placed.h);
    if cfg!(target_os = "linux") {
        let _ = window.set_resizable(true);
    }
    window.set_size(size).map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(placed.x, placed.y))
        .map_err(|e| e.to_string())?;
    window.set_size(size).map_err(|e| e.to_string())?;
    // macOS: Tauri's always-on-top means the floating level (5), below the
    // menu bar; the native panel level (27) is set by `pin_native` instead.
    if !cfg!(target_os = "macos") {
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
    }
    if settings.position == OverlayPosition::TopCenter {
        pin_native(&window, logical);
    }
    Ok(())
}

/// macOS: re-pin the frame natively so it sits over the menu bar and notch
/// (see `next_agent_macos_panel::pin_top_center`). No-op elsewhere.
#[cfg(target_os = "macos")]
fn pin_native(window: &WebviewWindow, logical: (f64, f64)) {
    let Ok(ns) = window.ns_window() else { return };
    let ns = ns as usize;
    let _ = window.run_on_main_thread(move || {
        let _ = next_agent_macos_panel::pin_top_center(
            ns as *mut std::ffi::c_void,
            logical.0,
            logical.1,
        );
    });
}

#[cfg(not(target_os = "macos"))]
fn pin_native(_window: &WebviewWindow, _logical: (f64, f64)) {}

/// macOS: panel-like behaviour and notch detection. Returns the notch.
#[cfg(target_os = "macos")]
fn native_panel(window: &WebviewWindow) -> Option<(f64, f64)> {
    let ns = window.ns_window().ok()? as usize;
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .run_on_main_thread(move || {
            let ptr = ns as *mut std::ffi::c_void;
            let ok = next_agent_macos_panel::configure_panel(ptr);
            let notch = next_agent_macos_panel::notch_of(ptr).map(|n| (n.width, n.height));
            let _ = tx.send((ok, notch));
        })
        .ok()?;
    rx.recv_timeout(Duration::from_secs(2))
        .ok()
        .and_then(|(_, n)| n)
}

#[cfg(not(target_os = "macos"))]
fn native_panel(_window: &WebviewWindow) -> Option<(f64, f64)> {
    None
}

fn create_window(
    app: &AppHandle,
    state: &OverlayHandle,
    service: &Arc<AppService>,
) -> Result<(), String> {
    if app.get_webview_window(LABEL).is_some() {
        return apply(app, state, service);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay".into()))
        .title("Next Agent overlay")
        .decorations(false)
        .transparent(true)
        .always_on_top(!cfg!(target_os = "macos"))
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .inner_size(COLLAPSED.0, COLLAPSED.1)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = window.set_visible_on_all_workspaces(true);
    // Click-through until the frontend reports the island rectangle.
    let _ = window.set_ignore_cursor_events(true);
    apply(app, state, service)?;
    if let Some(notch) = native_panel(&window) {
        if let Ok(mut inner) = state.inner.lock() {
            inner.notch = Some(notch);
        }
        apply(app, state, service)?;
    }
    window.show().map_err(|e| e.to_string())?;
    // Ordering the window in lets AppKit push it below the menu bar; put it
    // back over the notch.
    apply(app, state, service)?;
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    spawn_poller(app.clone(), Arc::clone(state), generation);
    Ok(())
}

fn close_window(app: &AppHandle, state: &OverlayState) {
    state.generation.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.close();
    }
}

/// Cursor poll: toggles `set_ignore_cursor_events` only on change.
fn spawn_poller(app: AppHandle, state: OverlayHandle, generation: u64) {
    let _ = std::thread::Builder::new()
        .name("overlay-poll".into())
        .spawn(move || {
            let mut machine = ClickThrough::default();
            while state.generation.load(Ordering::SeqCst) == generation {
                let Some(window) = app.get_webview_window(LABEL) else {
                    break;
                };
                let rect = state.inner.lock().ok().and_then(|i| i.rect);
                let scale = window.scale_factor().unwrap_or(1.0);
                let cursor = window
                    .cursor_position()
                    .ok()
                    .zip(window.outer_position().ok());
                let relative = cursor.map(|(c, w)| (c.x - f64::from(w.x), c.y - f64::from(w.y)));
                let tick = machine.tick(relative, rect, scale);
                if let Some(inside) = tick.changed {
                    let _ = window.set_ignore_cursor_events(!inside);
                    let _ = window.emit("overlay-pointer", inside);
                }
                std::thread::sleep(tick.next_poll);
            }
        });
}

/// Create the overlay at startup when enabled.
pub fn init(app: &AppHandle, service: &Arc<AppService>) {
    let state: OverlayHandle = Arc::default();
    app.manage(Arc::clone(&state));
    if load_settings(service).enabled {
        if let Err(error) = create_window(app, &state, service) {
            eprintln!("overlay: could not open ({error})");
        }
    }
}

// ----------------------------------------------------------------- commands --

type Svc<'a> = State<'a, Arc<AppService>>;
type Ov<'a> = State<'a, OverlayHandle>;

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
#[must_use]
pub fn overlay_info(app: AppHandle, service: Svc, overlay: Ov) -> OverlayInfo {
    OverlayInfo {
        settings: load_settings(&service),
        monitors: geoms(&app).into_iter().filter_map(|m| m.name).collect(),
        // macOS too: the `macos-private-api` feature allows a clear webview.
        transparent: true,
        notch: overlay.inner.lock().ok().and_then(|i| i.notch),
    }
}

/// Async so window creation never blocks the main thread (Windows deadlock).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub async fn overlay_set_settings(
    app: AppHandle,
    service: Svc<'_>,
    overlay: Ov<'_>,
    settings: OverlaySettings,
) -> Result<(), String> {
    save_settings(&service, &settings)?;
    if settings.enabled {
        create_window(&app, &overlay, &service)
    } else {
        close_window(&app, &overlay);
        Ok(())
    }
}

/// The island rectangle (CSS px, window-relative) used for click-through.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn overlay_set_rect(overlay: Ov, rect: Option<Rect>) -> Result<(), String> {
    overlay.inner.lock().map_err(|e| e.to_string())?.rect = rect;
    Ok(())
}

/// Grow or shrink the window (top-anchored).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn overlay_set_expanded(
    app: AppHandle,
    service: Svc,
    overlay: Ov,
    expanded: bool,
) -> Result<(), String> {
    {
        let mut inner = overlay.inner.lock().map_err(|e| e.to_string())?;
        if inner.expanded == expanded {
            return Ok(());
        }
        inner.expanded = expanded;
    }
    apply(&app, &overlay, &service)
}

/// Show and focus the main window on a task.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn overlay_open_task(app: AppHandle, task_id: String) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is not open".to_string())?;
    let _ = main.unminimize();
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit_to("main", "focus-task", task_id)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::TaskRequest;
    use std::sync::Mutex as StdMutex;

    fn mon(name: &str, x: i32, y: i32, w: u32, h: u32, scale: f64) -> MonitorGeom {
        MonitorGeom {
            name: Some(name.into()),
            x,
            y,
            w,
            h,
            work_x: x,
            work_y: y + 40,
            work_w: w,
            work_h: h - 40,
            scale,
        }
    }

    #[test]
    fn top_centre_on_primary() {
        let m = mon("A", 0, 0, 1920, 1080, 1.0);
        let p = place(&m, EXPANDED, OverlayPosition::TopCenter, false);
        assert_eq!(
            p,
            Placed {
                x: 600,
                y: 0,
                w: 720,
                h: 320
            }
        );
    }

    #[test]
    fn scales_and_follows_secondary_monitor_origin() {
        let m = mon("B", 1920, -200, 3000, 2000, 2.0);
        let p = place(&m, COLLAPSED, OverlayPosition::TopCenter, false);
        assert_eq!((p.w, p.h), (480, 56));
        assert_eq!(p.x, 1920 + (3000 - 480) / 2);
        assert_eq!(p.y, -200);
    }

    #[test]
    fn windows_offsets_below_work_area_top() {
        let m = mon("A", 0, 0, 1920, 1080, 1.0);
        assert_eq!(place(&m, COLLAPSED, OverlayPosition::TopCenter, true).y, 40);
        assert_eq!(place(&m, COLLAPSED, OverlayPosition::TopCenter, false).y, 0);
    }

    #[test]
    fn corners_keep_a_margin_and_stay_on_screen() {
        let m = mon("A", 100, 0, 1000, 800, 1.0);
        let left = place(&m, EXPANDED, OverlayPosition::TopLeft, false);
        let right = place(&m, EXPANDED, OverlayPosition::TopRight, false);
        assert_eq!(left.x, 116);
        assert_eq!(right.x, 100 + 1000 - 720 - 16);
        let tiny = mon("T", 0, 0, 500, 400, 1.0);
        let p = place(&tiny, EXPANDED, OverlayPosition::TopRight, false);
        assert_eq!((p.x, p.w), (0, 500), "never wider than the monitor");
    }

    #[test]
    fn collapsed_size_grows_around_a_notch() {
        assert_eq!(collapsed_size(None), COLLAPSED);
        assert_eq!(collapsed_size(Some((184.0, 32.0))), (324.0, 32.0));
    }

    #[test]
    fn monitor_choice_saved_then_cursor_then_primary() {
        let ms = [
            mon("A", 0, 0, 1920, 1080, 1.0),
            mon("B", 1920, 0, 1920, 1080, 1.0),
        ];
        assert_eq!(
            pick_monitor(&ms, Some("B"), Some((10.0, 10.0)), Some("A")),
            Some(1)
        );
        assert_eq!(
            pick_monitor(&ms, Some("gone"), Some((2000.0, 5.0)), Some("A")),
            Some(1)
        );
        assert_eq!(
            pick_monitor(&ms, None, Some((9999.0, 5.0)), Some("B")),
            Some(1)
        );
        assert_eq!(pick_monitor(&ms, None, None, None), Some(0));
        assert_eq!(pick_monitor(&[], None, None, None), None);
    }

    #[allow(clippy::unnecessary_wraps)]
    fn rect() -> Option<Rect> {
        Some(Rect {
            x: 100.0,
            y: 0.0,
            w: 200.0,
            h: 30.0,
        })
    }

    #[test]
    fn click_through_toggles_only_on_change() {
        let mut ct = ClickThrough::default();
        // First poll always reports the initial state (far: click-through).
        assert_eq!(
            ct.tick(Some((900.0, 900.0)), rect(), 1.0).changed,
            Some(false)
        );
        assert_eq!(ct.tick(Some((901.0, 900.0)), rect(), 1.0).changed, None);
        // Inside the island: capture.
        assert_eq!(
            ct.tick(Some((150.0, 10.0)), rect(), 1.0).changed,
            Some(true)
        );
        assert_eq!(ct.tick(Some((151.0, 10.0)), rect(), 1.0).changed, None);
        // 14 px margin: still captured just outside the rect, released beyond.
        assert_eq!(ct.tick(Some((310.0, 10.0)), rect(), 1.0).changed, None);
        assert_eq!(
            ct.tick(Some((315.0, 10.0)), rect(), 1.0).changed,
            Some(false)
        );
    }

    #[test]
    fn click_through_polls_fast_near_and_slow_far() {
        let mut ct = ClickThrough::default();
        assert_eq!(
            ct.tick(Some((350.0, 10.0)), rect(), 1.0).next_poll,
            POLL_NEAR
        );
        assert_eq!(
            ct.tick(Some((900.0, 500.0)), rect(), 1.0).next_poll,
            POLL_FAR
        );
        assert_eq!(ct.tick(None, rect(), 1.0).next_poll, POLL_FAR);
    }

    #[test]
    fn click_through_scales_css_pixels_and_ignores_when_no_rect() {
        let mut ct = ClickThrough::default();
        // 2x scale: rect spans physical x 200..600.
        assert_eq!(
            ct.tick(Some((250.0, 20.0)), rect(), 2.0).changed,
            Some(true)
        );
        let mut none = ClickThrough::default();
        assert_eq!(
            none.tick(Some((250.0, 20.0)), None, 2.0).changed,
            Some(false)
        );
    }

    #[test]
    fn settings_default_and_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = AppService::new(dir.path().join("db.sqlite")).expect("service");
        assert_eq!(load_settings(&svc), OverlaySettings::default());
        let s = OverlaySettings {
            enabled: false,
            position: OverlayPosition::TopRight,
            monitor: Some("DELL".into()),
        };
        save_settings(&svc, &s).expect("save");
        assert_eq!(load_settings(&svc), s);
    }

    /// An approval answered through the same path the overlay's buttons use
    /// (`approval_respond`) resolves the running task, and the broadcast
    /// carries the events the overlay renders.
    #[test]
    fn overlay_answer_resolves_running_task_and_broadcast_sees_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("service"));
        let ws = dir.path().join("plain");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");

        let seen: Arc<StdMutex<Vec<StreamMessage>>> = Arc::default();
        let log = Arc::clone(&seen);
        let broadcast: Sink = Arc::new(move |m| log.lock().expect("lock").push(m));
        // "Overlay": answers the first approval it sees on the broadcast.
        let responder = Arc::clone(&svc);
        let overlay_sink: Sink = Arc::new(move |m| {
            if let StreamMessage::Event { event } = &m {
                if event["kind"] == "approval_requested" {
                    let task = event["task_id"].as_str().expect("task");
                    let id = event["data"]["approval_id"].as_str().expect("id");
                    responder
                        .approval_respond(task, id, "allow")
                        .expect("respond");
                }
            }
        });
        let sink = fanout(
            Arc::new(|_| {}),
            Arc::new(move |m| {
                broadcast(m.clone());
                overlay_sink(m);
            }),
        );
        let request = TaskRequest {
            workspace_id: opened.id,
            prompt: "Add a demo note".into(),
            mode: "manual".into(),
            profile: None,
        };
        let id = svc.task_start(&request, sink).expect("start");
        svc.task_wait(&id);

        let messages = seen.lock().expect("lock");
        let kinds: Vec<String> = messages
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Event { event } => event["kind"].as_str().map(str::to_string),
                _ => None,
            })
            .collect();
        assert!(kinds.contains(&"approval_requested".to_string()));
        assert!(kinds.contains(&"approval_resolved".to_string()));
        assert!(kinds.contains(&"task_completed".to_string()), "{kinds:?}");
        assert!(ws.join("NEXT_AGENT_DEMO.md").exists(), "allowed write ran");
    }

    #[test]
    fn fanout_broadcasts_events_only() {
        let direct: Arc<StdMutex<usize>> = Arc::default();
        let cast: Arc<StdMutex<usize>> = Arc::default();
        let (d, c) = (Arc::clone(&direct), Arc::clone(&cast));
        let sink = fanout(
            Arc::new(move |_| *d.lock().expect("lock") += 1),
            Arc::new(move |_| *c.lock().expect("lock") += 1),
        );
        sink(StreamMessage::Event {
            event: serde_json::json!({}),
        });
        sink(StreamMessage::Documented {
            task_id: "t".into(),
            path: "p".into(),
        });
        assert_eq!(
            (*direct.lock().expect("lock"), *cast.lock().expect("lock")),
            (2, 1)
        );
    }
}
