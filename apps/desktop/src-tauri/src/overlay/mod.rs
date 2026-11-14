// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandWindowController.swift and windows/src-tauri/src/island.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8

//! The overlay island: a second, click-through Tauri window that wraps the
//! notch (or sits at the top of the screen) and shows the active task and
//! pending approvals.
//!
//! A port of Coucou's island (MIT, © 2026 Louis Raillé, commit
//! `0d7fe58884d99f275f3238e8dbe29ddee69fa9e8`; see `NOTICE.md`), from
//! `NotchBuddy/Sources/App/IslandWindowController.swift` (macOS) and
//! `windows/src-tauri/src/island.rs` (Windows and Linux):
//!
//! - one fixed-size transparent panel at the top of the chosen screen
//!   (720×560 on macOS, 720×320 on a PC, as in Coucou); the island is drawn
//!   inside it by the page and animates there; outside the island every
//!   click goes through. On a PC a hidden island folds the window to a
//!   240×6 wake strip (`island.rs` `apply_geometry`);
//! - macOS: the panel is Coucou's `IslandPanel` (`next_agent_macos_panel`),
//!   a non-activating `NSPanel` at `mainMenu + 3` placed with
//!   `setFrame:display:` over the notch;
//! - Windows/Linux: [`geometry`] places it (work-area offset on Windows,
//!   layer-shell or a Dock window on Linux, [`platform`]);
//! - click-through: an adaptive cursor poll ([`poll`]) on macOS and Windows,
//!   the input region on Linux;
//! - screens re-checked twice a second: plugged, unplugged, rearranged or
//!   rescaled screens reposition the panel, and follow-the-mouse moves it.
//!
//! Task events reach the page as the `task-stream` event ([`fanout`]);
//! approvals are answered through the existing `approval_respond` command.

pub mod geometry;
pub mod platform;
pub mod poll;

use crate::service::{AppService, Sink};
use crate::wire::StreamMessage;
#[cfg(not(target_os = "macos"))]
use geometry::describe;
use geometry::{target_monitor, MonitorGeom, PC_COMPACT_EXTRA, PC_REST_H, PC_REST_W};
#[cfg(not(target_os = "macos"))]
use next_agent_macos_panel::clamp_collapsed;
use next_agent_macos_panel::{
    DisplayChoice, Placement, Rect, PANEL_H, PANEL_W, PC_PANEL_H, PC_PANEL_W,
};
use poll::{ClickThrough, PollInput};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::{PhysicalPosition, PhysicalSize};

pub const LABEL: &str = "overlay";
const SETTINGS_KEY: &str = "overlay.settings";
/// Compact island on a notch Mac: the notch plus two 80 pt wings
/// (`islandSize(.compact)` = `nw + 160`).
const MAC_COMPACT_EXTRA: f64 = 160.0;
/// How often the screens are re-checked.
const SCREEN_CHECK: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------- settings --

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    #[default]
    TopCenter,
    TopLeft,
    TopRight,
}

impl OverlayPosition {
    #[must_use]
    pub fn placement(self) -> Placement {
        match self {
            Self::TopCenter => Placement::Notch,
            Self::TopLeft => Placement::TopLeft,
            Self::TopRight => Placement::TopRight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub position: OverlayPosition,
    /// [`DisplayChoice`] storage value (`notch`, `menuBar`, `followMouse`,
    /// `display:<id>`); `None` is the default (the notch screen, else the
    /// main/primary one).
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

impl OverlaySettings {
    #[must_use]
    pub fn display_choice(&self) -> DisplayChoice {
        self.monitor
            .as_deref()
            .map_or_else(DisplayChoice::default, DisplayChoice::from_storage)
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

// -------------------------------------------------------------- island info --

/// What the page needs to draw the island on the current screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandInfo {
    /// The screen has a notch the island wraps.
    pub has_notch: bool,
    /// Resting island (the notch, or the no-notch bar), logical px.
    pub width: f64,
    pub height: f64,
    /// Compact island = resting width + this (the two wings).
    pub compact_extra: f64,
    pub panel: (f64, f64),
    pub position: OverlayPosition,
    /// `macos`, `windows` or `linux`.
    pub platform: &'static str,
}

impl IslandInfo {
    fn pc(position: OverlayPosition) -> Self {
        Self {
            has_notch: false,
            width: PC_REST_W,
            height: PC_REST_H,
            compact_extra: PC_COMPACT_EXTRA,
            panel: (PC_PANEL_W, PC_PANEL_H),
            position,
            platform: if cfg!(windows) { "windows" } else { "linux" },
        }
    }
}

/// One entry of the monitor list in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonitorOption {
    /// A [`DisplayChoice`] storage value.
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    pub settings: OverlaySettings,
    pub monitors: Vec<MonitorOption>,
    pub island: IslandInfo,
}

// ------------------------------------------------------------ window runtime --

#[derive(Debug, Default)]
struct Inner {
    /// The island rectangle the page draws (CSS px = logical px, relative
    /// to the panel's top-left).
    rect: Option<Rect>,
    expanded: bool,
    /// PC: the window folded to this size while the island is hidden.
    collapsed: Option<(f64, f64)>,
    island: Option<IslandInfo>,
}

/// Managed overlay state.
#[derive(Debug, Default)]
pub struct OverlayState {
    inner: Mutex<Inner>,
    /// Bumped on every window (re)creation and close so stale pollers exit.
    generation: Arc<AtomicU64>,
}

pub type OverlayHandle = Arc<OverlayState>;

impl OverlayState {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }
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

/// Run `f` on the main thread and wait for its result (immediately when
/// already there: Tauri runs main-thread tasks inline on the main thread).
fn on_main<R: Send + 'static>(
    window: &WebviewWindow,
    f: impl FnOnce(&WebviewWindow) -> R + Send + 'static,
) -> Option<R> {
    let (tx, rx) = std::sync::mpsc::channel();
    let w = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(&w));
        })
        .ok()?;
    rx.recv_timeout(Duration::from_secs(2)).ok()
}

#[cfg(target_os = "macos")]
fn ns_ptr(window: &WebviewWindow) -> Option<usize> {
    window.ns_window().ok().map(|p| p as usize)
}

/// macOS: Coucou's `relocate(to:)` — the native frame over the notch.
#[cfg(target_os = "macos")]
fn place_window(
    _app: &AppHandle,
    window: &WebviewWindow,
    settings: &OverlaySettings,
    _collapsed: Option<(f64, f64)>,
) -> Option<IslandInfo> {
    let choice = settings.display_choice();
    let position = settings.position;
    on_main(window, move |_| {
        next_agent_macos_panel::place(&choice, position.placement(), (PANEL_W, PANEL_H))
    })
    .flatten()
    .map(|placed| IslandInfo {
        has_notch: placed.geometry.has_notch,
        width: placed.geometry.width,
        height: placed.geometry.height,
        compact_extra: MAC_COMPACT_EXTRA,
        panel: (PANEL_W, PANEL_H),
        position,
        platform: "macos",
    })
}

/// Windows/Linux: Coucou's `apply_geometry` — size → position → pin → size →
/// always-on-top (moving across displays can rescale the window; GTK will
/// not shrink a non-resizable window), then one retry on a size mismatch.
#[cfg(not(target_os = "macos"))]
fn place_window(
    app: &AppHandle,
    window: &WebviewWindow,
    settings: &OverlaySettings,
    collapsed: Option<(f64, f64)>,
) -> Option<IslandInfo> {
    let monitors = geoms(app);
    let cursor = app.cursor_position().ok().map(|p| (p.x, p.y));
    let primary = app.primary_monitor().ok().flatten().and_then(|p| {
        monitors
            .iter()
            .position(|m| m.x == p.position().x && m.y == p.position().y)
    });
    let index = target_monitor(&monitors, &settings.display_choice(), cursor, primary)?;
    let m = &monitors[index];
    let placement = settings.position.placement();
    let size = collapsed
        .map(clamp_collapsed)
        .unwrap_or((PC_PANEL_W, PC_PANEL_H));
    let placed = geometry::place(m, placement, size, cfg!(windows));
    let size = PhysicalSize::new(placed.w, placed.h);
    if cfg!(target_os = "linux") {
        let _ = window.set_resizable(true);
    }
    let _ = window.set_size(size);
    let _ = window.set_position(PhysicalPosition::new(placed.x, placed.y));
    let id = describe(m);
    let w = window.clone();
    let _ = window.run_on_main_thread(move || platform::pin_to_monitor(&w, id.x, id.y, placement));
    let _ = window.set_size(size);
    let _ = window.set_always_on_top(true);
    if let Ok(got) = window.inner_size() {
        if got != size {
            let _ = window.set_size(size);
            let _ = window.set_position(PhysicalPosition::new(placed.x, placed.y));
        }
    }
    Some(IslandInfo::pc(settings.position))
}

/// Place the panel and tell the page when the island's geometry changed.
fn apply(app: &AppHandle, state: &OverlayState, service: &AppService) -> Result<(), String> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "overlay window is not open".to_string())?;
    let settings = load_settings(service);
    let collapsed = state.with(|i| i.collapsed);
    let island =
        place_window(app, &window, &settings, collapsed).ok_or("no screen for the overlay")?;
    let changed = state.with(|inner| {
        let changed = inner.island.as_ref() != Some(&island);
        inner.island = Some(island.clone());
        changed
    });
    if changed {
        let _ = window.emit("overlay-geometry", &island);
    }
    Ok(())
}

/// The monitor layout (and, following the mouse while folded, the cursor's
/// monitor). Any change means the panel has to be placed again.
fn screen_key(app: &AppHandle, follow: Option<(f64, f64)>) -> Vec<i64> {
    let monitors = geoms(app);
    let mut key: Vec<i64> = monitors
        .iter()
        .flat_map(|m| {
            [
                i64::from(m.x),
                i64::from(m.y),
                i64::from(m.w),
                i64::from(m.h),
                i64::from(m.work_y),
                i64::from(m.work_h),
                // Scale in thousandths is precise enough to notice a change.
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let s = (m.scale * 1000.0).round() as i64;
                    s
                },
            ]
        })
        .collect();
    if let Some(cursor) = follow {
        let index = target_monitor(&monitors, &DisplayChoice::FollowMouse, Some(cursor), None);
        key.push(index.map_or(-1, |i| i64::try_from(i).unwrap_or(-1)));
    }
    key
}

#[cfg(target_os = "macos")]
fn display_asleep() -> bool {
    next_agent_macos_panel::display_asleep()
}

#[cfg(not(target_os = "macos"))]
fn display_asleep() -> bool {
    false
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
        .resizable(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        // A click on Allow lands on the first click, without focusing first.
        .accept_first_mouse(true)
        .inner_size(
            if cfg!(target_os = "macos") {
                PANEL_W
            } else {
                PC_PANEL_W
            },
            if cfg!(target_os = "macos") {
                PANEL_H
            } else {
                PC_PANEL_H
            },
        )
        .build()
        .map_err(|e| e.to_string())?;
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let alive = Arc::clone(&state.generation);
    let placement = load_settings(service).position.placement();
    let _ = on_main(&window, move |w| {
        platform::make_overlay_window(w, placement, alive, generation);
    });
    // Click-through until the page reports the island.
    set_click_through(&window, true);
    state.with(|inner| {
        inner.rect = None;
        inner.island = None;
        inner.collapsed = None;
    });
    show(&window);
    apply(app, state, service)?;
    spawn_poller(
        app.clone(),
        Arc::clone(state),
        Arc::clone(service),
        generation,
    );
    Ok(())
}

/// macOS: Coucou's panel. The webview moves into a non-activating
/// `IslandPanel`, which is ordered in without becoming key (Tauri's `show()`
/// would make it key); the Tauri window stays hidden. Elsewhere: `show()`.
#[cfg(target_os = "macos")]
fn show(window: &WebviewWindow) {
    let Some(ns) = ns_ptr(window) else { return };
    let result = on_main(window, move |_| {
        next_agent_macos_panel::adopt(ns as *mut std::ffi::c_void, (PANEL_W, PANEL_H))?;
        if next_agent_macos_panel::order_front() {
            Ok(())
        } else {
            Err("could not order the panel in".to_string())
        }
    })
    .unwrap_or_else(|| Err("the main thread did not answer".into()));
    if let Err(why) = result {
        eprintln!("overlay: the island panel could not be created ({why})");
    }
}

#[cfg(not(target_os = "macos"))]
fn show(window: &WebviewWindow) {
    let _ = window.show();
}

fn close_window(app: &AppHandle, state: &OverlayState) {
    state.generation.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        #[cfg(target_os = "macos")]
        let _ = on_main(&window, |_| next_agent_macos_panel::release());
        let _ = window.close();
    }
}

/// The pointer relative to the panel's top-left, in logical px.
#[cfg(target_os = "macos")]
fn panel_cursor(_app: &AppHandle, _window: &WebviewWindow) -> Option<(f64, f64)> {
    next_agent_macos_panel::cursor_in_panel()
}

#[cfg(not(target_os = "macos"))]
fn panel_cursor(app: &AppHandle, window: &WebviewWindow) -> Option<(f64, f64)> {
    let scale = window.scale_factor().unwrap_or(1.0);
    let cursor = app.cursor_position().ok()?;
    let origin = window.outer_position().ok()?;
    Some((
        (cursor.x - f64::from(origin.x)) / scale,
        (cursor.y - f64::from(origin.y)) / scale,
    ))
}

/// Click-through on or off (macOS: the panel's `ignoresMouseEvents`).
#[cfg(target_os = "macos")]
fn set_click_through(window: &WebviewWindow, ignore: bool) {
    let _ = window.run_on_main_thread(move || {
        let _ = next_agent_macos_panel::set_ignores_mouse(ignore);
    });
}

#[cfg(not(target_os = "macos"))]
fn set_click_through(window: &WebviewWindow, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

/// Coucou's `pollFrame` loop on a thread: click-through from the cursor
/// (macOS, Windows), the screen check, and the display-sleep pause.
fn spawn_poller(app: AppHandle, state: OverlayHandle, service: Arc<AppService>, generation: u64) {
    let _ = std::thread::Builder::new()
        .name("overlay-poll".into())
        .spawn(move || {
            let mut machine = ClickThrough::default();
            let mut last_screen: Option<Vec<i64>> = None;
            let mut checked = Instant::now()
                .checked_sub(SCREEN_CHECK)
                .unwrap_or_else(Instant::now);
            while state.generation.load(Ordering::SeqCst) == generation {
                let Some(window) = app.get_webview_window(LABEL) else {
                    break;
                };
                let (rect, expanded, island) =
                    state.with(|i| (i.rect, i.expanded, i.island.clone()));
                let cursor = app.cursor_position().ok().map(|p| (p.x, p.y));

                if checked.elapsed() >= SCREEN_CHECK {
                    checked = Instant::now();
                    let follow = (!expanded
                        && load_settings(&service).display_choice() == DisplayChoice::FollowMouse)
                        .then_some(cursor)
                        .flatten();
                    let now = screen_key(&app, follow);
                    if last_screen.as_ref().is_some_and(|k| *k != now) {
                        let _ = apply(&app, &state, &service);
                        machine.reset();
                    }
                    last_screen = Some(now);
                }

                if !platform::CURSOR_POLL {
                    std::thread::sleep(SCREEN_CHECK);
                    continue;
                }
                let tick = machine.tick(&PollInput {
                    cursor: panel_cursor(&app, &window),
                    island: rect,
                    panel: island.as_ref().map_or((PANEL_W, PANEL_H), |i| i.panel),
                    expanded,
                    bar_without_notch: island.as_ref().is_some_and(|i| !i.has_notch),
                    display_asleep: display_asleep(),
                });
                if let Some(inside) = tick.changed {
                    set_click_through(&window, !inside);
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

/// Settings list: macOS screens by display UUID, PC monitors by key.
fn monitor_options(app: &AppHandle) -> Vec<MonitorOption> {
    #[cfg(target_os = "macos")]
    {
        let screens = app
            .get_webview_window("main")
            .or_else(|| app.get_webview_window(LABEL))
            .and_then(|w| on_main(&w, |_| next_agent_macos_panel::screens()))
            .unwrap_or_default();
        screens
            .into_iter()
            .filter_map(|s| {
                let id = s.id?;
                Some(MonitorOption {
                    key: DisplayChoice::Display(id).storage_value(),
                    label: if s.has_notch {
                        format!("{} (notch)", s.name)
                    } else {
                        s.name
                    },
                })
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        geoms(app)
            .iter()
            .map(|m| {
                let id = describe(m);
                MonitorOption {
                    key: DisplayChoice::Display(id.key()).storage_value(),
                    label: id.label(),
                }
            })
            .collect()
    }
}

// ----------------------------------------------------------------- commands --

type Svc<'a> = State<'a, Arc<AppService>>;
type Ov<'a> = State<'a, OverlayHandle>;

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub async fn overlay_info(
    app: AppHandle,
    service: Svc<'_>,
    overlay: Ov<'_>,
) -> Result<OverlayInfo, String> {
    let settings = load_settings(&service);
    let island = overlay
        .with(|i| i.island.clone())
        .unwrap_or_else(|| IslandInfo::pc(settings.position));
    Ok(OverlayInfo {
        monitors: monitor_options(&app),
        settings,
        island,
    })
}

/// Applied live: on/off, position and monitor take effect at once.
/// Async so window creation never blocks the main thread (Windows deadlock).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub async fn overlay_set_settings(
    app: AppHandle,
    service: Svc<'_>,
    overlay: Ov<'_>,
    settings: OverlaySettings,
) -> Result<(), String> {
    let before = load_settings(&service);
    save_settings(&service, &settings)?;
    if !settings.enabled {
        close_window(&app, &overlay);
        return Ok(());
    }
    // Linux: a layer surface's anchors are set before it is first shown, so a
    // new position re-creates it there.
    if cfg!(target_os = "linux") && before.position != settings.position {
        close_window(&app, &overlay);
        // Let the old window go before a new one takes the label.
        std::thread::sleep(Duration::from_millis(150));
    }
    create_window(&app, &overlay, &service)
}

/// The island rectangle as the page sends it (CSS px, panel-relative).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct IslandRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The island rectangle used for click-through.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn overlay_set_rect(
    app: AppHandle,
    overlay: Ov,
    rect: Option<IslandRect>,
) -> Result<(), String> {
    let rect = rect.map(|r| Rect::new(r.x, r.y, r.w, r.h));
    overlay.with(|i| i.rect = rect);
    if !platform::CURSOR_POLL {
        if let Some(window) = app.get_webview_window(LABEL) {
            let region = rect.map_or(Some((0.0, 0.0, 0.0, 0.0)), |r| {
                let m = poll::HIT_MARGIN;
                let (x0, y0) = ((r.x - m).max(0.0), (r.y - m).max(0.0));
                Some((x0, y0, r.max_x() + m - x0, r.max_y() + m - y0))
            });
            let w = window.clone();
            let _ = window.run_on_main_thread(move || platform::set_input_region(&w, region));
        }
    }
    Ok(())
}

/// The island opened or folded (sets the poll rate; the window keeps its size).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub fn overlay_set_expanded(overlay: Ov, expanded: bool) -> Result<(), String> {
    overlay.with(|i| i.expanded = expanded);
    Ok(())
}

/// PC: fold the window to the hidden island (`Some(size)`, the wake strip
/// when nothing is drawn) or grow it back to the panel (`None`), as Coucou's
/// `set_collapsed`. macOS keeps its panel (the hidden island sits in the notch).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]
pub async fn overlay_set_collapsed(
    app: AppHandle,
    service: Svc<'_>,
    overlay: Ov<'_>,
    size: Option<(f64, f64)>,
) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    overlay.with(|i| i.collapsed = size);
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

    #[test]
    fn settings_default_round_trip_and_display_choice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = AppService::new(dir.path().join("db.sqlite")).expect("service");
        assert_eq!(load_settings(&svc), OverlaySettings::default());
        assert_eq!(
            OverlaySettings::default().display_choice(),
            DisplayChoice::Notch
        );
        let s = OverlaySettings {
            enabled: false,
            position: OverlayPosition::TopRight,
            monitor: Some("display:37D8832A".into()),
        };
        save_settings(&svc, &s).expect("save");
        assert_eq!(load_settings(&svc), s);
        assert_eq!(
            s.display_choice(),
            DisplayChoice::Display("37D8832A".into())
        );
        assert_eq!(s.position.placement(), Placement::TopRight);
        // A monitor name saved by the previous overlay is not a choice: default.
        let old = OverlaySettings {
            monitor: Some("DELL U2720Q".into()),
            ..OverlaySettings::default()
        };
        assert_eq!(old.display_choice(), DisplayChoice::Notch);
    }

    #[test]
    fn island_info_serialises_for_the_page() {
        let json = serde_json::to_value(IslandInfo::pc(OverlayPosition::TopCenter)).expect("json");
        assert_eq!(json["hasNotch"], false);
        assert_eq!(json["width"], 184.0);
        assert_eq!(json["compactExtra"], 104.0);
        assert_eq!(json["position"], "top-center");
        assert_eq!(json["panel"][0], 720.0);
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
