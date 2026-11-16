//! The diagnostics log in the desktop app (ADR-0013,
//! docs/architecture/Logging.md): started first thing in `run`, and the five
//! `diag_*` commands for Settings → Diagnostics (main window only).
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use crate::service::AppService;
use next_agent_diagnostics::{
    self as diag, parse_level, Config, ExportSummary, Handle, Level, LogEntry, RecentFilter,
};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

/// The saved setting holding the level (`error`, `warn`, `info` or `debug`).
pub const LEVEL_SETTING: &str = "logging.level";
/// `diag_log` calls accepted per second; the rest are dropped.
const UI_EVENTS_PER_SECOND: u32 = 20;
const UNAVAILABLE: &str = "the diagnostics log is not available";

/// The running log (absent when its folder could not be created) and the
/// UI's rate limit.
pub struct Diagnostics {
    handle: Option<Handle>,
    limiter: RateLimiter,
}

/// Start the log in `<app data>/logs`, at `info` until the saved level is
/// read ([`apply_saved_level`]). Never fails: without a log the app still
/// runs, and the commands report the log as unavailable.
#[must_use]
pub fn start() -> Diagnostics {
    let handle = dirs::data_dir()
        .map(|data| diag::logs_dir(&data.join(crate::APP_IDENTIFIER)))
        .ok_or_else(|| "no app data folder".to_string())
        .and_then(|dir| {
            let mut config = Config::new(dir);
            config.stderr = cfg!(debug_assertions);
            diag::init(config).map_err(|e| e.to_string())
        })
        .map_err(|error| eprintln!("diagnostics log not started: {error}"))
        .ok();
    tracing::info!(
        target: "app",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "app_started"
    );
    Diagnostics {
        handle,
        limiter: RateLimiter::new(UI_EVENTS_PER_SECOND),
    }
}

/// Apply the level saved in Settings (default `info`).
pub fn apply_saved_level(diagnostics: &Diagnostics, service: &AppService) {
    let Some(handle) = &diagnostics.handle else {
        return;
    };
    let saved = service.setting_get(LEVEL_SETTING).ok().flatten();
    handle.set_level(
        saved
            .as_deref()
            .and_then(parse_level)
            .unwrap_or(Level::INFO),
    );
}

impl Diagnostics {
    fn handle(&self) -> Result<&Handle, String> {
        self.handle.as_ref().ok_or_else(|| UNAVAILABLE.to_string())
    }
}

fn level_arg(name: &str) -> Result<Level, String> {
    parse_level(name).ok_or_else(|| format!("unknown level `{name}` (error, warn, info or debug)"))
}

/// Recent entries from memory, newest first. `level` keeps that level and
/// more severe ones; `target` keeps one area.
#[tauri::command]
pub fn diag_recent(
    diagnostics: State<Diagnostics>,
    level: Option<String>,
    target: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<LogEntry>, String> {
    let filter = RecentFilter {
        level: level.as_deref().map(level_arg).transpose()?,
        target,
        limit,
    };
    Ok(diagnostics.handle()?.recent(&filter))
}

/// Change the level at once and save it as `logging.level`.
#[tauri::command]
pub fn diag_set_level(
    diagnostics: State<Diagnostics>,
    service: State<Arc<AppService>>,
    level: String,
) -> Result<(), String> {
    let parsed = level_arg(&level)?;
    let handle = diagnostics.handle()?;
    service.setting_set(LEVEL_SETTING, &level)?;
    handle.set_level(parsed);
    tracing::info!(target: "app", setting = LEVEL_SETTING, "setting_changed");
    Ok(())
}

/// Show the logs folder in the file manager.
#[tauri::command]
pub fn diag_open_folder(diagnostics: State<Diagnostics>) -> Result<(), String> {
    crate::open_with_os(diagnostics.handle()?.folder())
        .map_err(|e| format!("could not open the logs folder: {e}"))
}

/// Write a redacted copy of every log file to Downloads, for a bug report.
#[tauri::command]
pub fn diag_export(
    app: AppHandle,
    diagnostics: State<Diagnostics>,
) -> Result<ExportSummary, String> {
    let downloads = app
        .path()
        .download_dir()
        .map_err(|e| format!("no Downloads folder: {e}"))?;
    let summary = diagnostics
        .handle()?
        .export(&downloads.join(diag::export_file_name()))?;
    tracing::info!(target: "app", count = summary.entries, "log_exported");
    Ok(summary)
}

/// An event from the UI (`lib/log.ts`). At most 20 a second; fields go
/// through the same allowlist and redaction as Rust events.
#[tauri::command]
pub fn diag_log(
    diagnostics: State<Diagnostics>,
    level: String,
    event: String,
    fields: Option<Map<String, Value>>,
) -> Result<(), String> {
    let level = level_arg(&level)?;
    if !diagnostics.limiter.allow(Instant::now()) {
        return Ok(());
    }
    diagnostics
        .handle()?
        .record(level, &event, &fields.unwrap_or_default())
        .map(|_| ())
}

/// At most `per_second` calls in each one-second window.
struct RateLimiter {
    per_second: u32,
    window: Mutex<(Option<Instant>, u32)>,
}

impl RateLimiter {
    fn new(per_second: u32) -> Self {
        Self {
            per_second,
            window: Mutex::new((None, 0)),
        }
    }

    fn allow(&self, now: Instant) -> bool {
        let mut window = self.window.lock().unwrap_or_else(PoisonError::into_inner);
        let (start, count) = &mut *window;
        if start.is_none_or(|s| now.duration_since(s) >= Duration::from_secs(1)) {
            *start = Some(now);
            *count = 0;
        }
        *count += 1;
        *count <= self.per_second
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_20_a_second() {
        let limiter = RateLimiter::new(UI_EVENTS_PER_SECOND);
        let t0 = Instant::now();
        let allowed = (0..30).filter(|_| limiter.allow(t0)).count();
        assert_eq!(allowed, 20);
        assert!(!limiter.allow(t0 + Duration::from_millis(999)));
        assert!(limiter.allow(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn levels_are_checked() {
        assert_eq!(level_arg("warn"), Ok(Level::WARN));
        assert!(level_arg("trace").is_err());
        assert!(level_arg("verbose").is_err());
    }

    #[test]
    fn the_level_setting_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let service = AppService::new(dir.path().join("db.sqlite")).unwrap();
        assert_eq!(service.setting_get(LEVEL_SETTING).unwrap(), None);
        service.setting_set(LEVEL_SETTING, "debug").unwrap();
        assert_eq!(
            service.setting_get(LEVEL_SETTING).unwrap().as_deref(),
            Some("debug")
        );
    }
}
