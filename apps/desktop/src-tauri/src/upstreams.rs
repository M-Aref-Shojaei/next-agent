//! Settings → Upstreams: the background upstream watch and its commands.
//!
//! Once a day while the app runs (and on "Check now"), asks GitHub's public
//! API what each watched upstream published since its pin
//! (`next_agent_core::upstream`). It only reports: nothing is downloaded,
//! applied or merged. The last result is kept in `upstream-watch.json` in the
//! app's data folder, so a restart does not check again straight away.
#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::must_use_candidate
)] // Tauri commands take owned args.

use next_agent_core::upstream::github::{check_all, GithubClient, API_BASE};
use next_agent_core::upstream::{self, UpstreamReport, WatchState, Watched};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

/// The event the main window listens to.
pub const EVENT: &str = "upstream-watch";

/// How often the background task wakes to see whether a check is due.
const TICK: Duration = Duration::from_hours(1);

/// What Settings → Upstreams shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub watched: Vec<Watched>,
    pub reports: Vec<UpstreamReport>,
    pub last_check: Option<u64>,
    pub rate_limited_until: Option<u64>,
    /// Ids with something not yet marked as seen (the badge).
    pub unseen: Vec<String>,
    pub checking: bool,
    /// The watch list itself could not be read.
    pub error: Option<String>,
}

/// The watch's state, managed by Tauri.
pub struct UpstreamWatch {
    path: PathBuf,
    state: Mutex<WatchState>,
    watched: Result<Vec<Watched>, String>,
    /// Held while a check runs, so two never overlap.
    running: tokio::sync::Mutex<()>,
    checking: std::sync::atomic::AtomicBool,
    api_base: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn load(path: &Path) -> WatchState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(path: &Path, state: &WatchState) {
    let Ok(text) = serde_json::to_string_pretty(state) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

impl UpstreamWatch {
    /// The watch for the app data folder `dir`, against `api_base`.
    #[must_use]
    pub fn new(dir: &Path, api_base: &str) -> Self {
        let path = dir.join("upstream-watch.json");
        let mut state = load(&path);
        let watched = upstream::watched();
        // Drop reports of upstreams no longer watched or re-pinned since.
        if let Ok(list) = &watched {
            state
                .reports
                .retain(|r| list.iter().any(|w| w.id == r.id && w.commit == r.pinned));
        }
        Self {
            path,
            state: Mutex::new(state),
            watched,
            running: tokio::sync::Mutex::new(()),
            checking: std::sync::atomic::AtomicBool::new(false),
            api_base: api_base.to_string(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, WatchState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What the settings page shows.
    #[must_use]
    pub fn view(&self) -> WatchView {
        let state = self.lock().clone();
        WatchView {
            watched: self.watched.clone().unwrap_or_default(),
            unseen: state.unseen(),
            reports: state.reports,
            last_check: state.last_check,
            rate_limited_until: state.rate_limited_until,
            checking: self.checking.load(std::sync::atomic::Ordering::Relaxed),
            error: self.watched.clone().err(),
        }
    }

    /// Run a check now unless one is running or GitHub asked us to wait.
    /// `force` skips the once-a-day rule (not the rate limit).
    pub async fn check(&self, force: bool) -> WatchView {
        let Ok(list) = self.watched.clone() else {
            return self.view();
        };
        let Ok(_guard) = self.running.try_lock() else {
            return self.view();
        };
        let (previous, go) = {
            let state = self.lock();
            let go = if force {
                !state.rate_limited(now())
            } else {
                state.due(now())
            };
            (state.reports.clone(), go)
        };
        if !go || list.is_empty() {
            return self.view();
        }
        self.checking
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let result = match GithubClient::new(&self.api_base) {
            Ok(client) => Ok(check_all(&client, &list, &previous, now()).await),
            Err(error) => Err(error),
        };
        self.checking
            .store(false, std::sync::atomic::Ordering::Relaxed);
        {
            let mut state = self.lock();
            match result {
                Ok((reports, limited)) => {
                    state.reports = reports;
                    state.rate_limited_until = limited;
                    // A rate-limited run is not a completed check.
                    if limited.is_none() {
                        state.last_check = Some(now());
                    }
                }
                Err(error) => {
                    for report in &mut state.reports {
                        report.error = Some(error.clone());
                    }
                }
            }
            save(&self.path, &state);
        }
        self.view()
    }

    /// The owner looked at everything currently listed.
    pub fn mark_seen(&self) -> WatchView {
        {
            let mut state = self.lock();
            state.mark_seen();
            save(&self.path, &state);
        }
        self.view()
    }

    /// Whether `url` is a GitHub page of a watched upstream (the only links
    /// the page may ask to open).
    #[must_use]
    pub fn may_open(&self, url: &str) -> bool {
        let Ok(list) = &self.watched else {
            return false;
        };
        list.iter().any(|w| {
            let prefix = format!("https://github.com/{}/", w.repo);
            url.starts_with(&prefix)
                && url
                    .chars()
                    .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '`'))
        })
    }
}

/// Manage the watch and start its daily background check.
pub fn start(app: &AppHandle, dir: &Path) -> Arc<UpstreamWatch> {
    let watch = Arc::new(UpstreamWatch::new(dir, API_BASE));
    let background = Arc::clone(&watch);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let before = background.view();
            let after = background.check(false).await;
            if after != before {
                let _ = handle.emit(EVENT, &after);
            }
            tokio::time::sleep(TICK).await;
        }
    });
    watch
}

type Watch<'a> = State<'a, Arc<UpstreamWatch>>;

#[tauri::command]
pub fn upstream_watch_status(watch: Watch) -> WatchView {
    watch.view()
}

#[tauri::command]
pub async fn upstream_watch_check(app: AppHandle, watch: Watch<'_>) -> Result<WatchView, String> {
    let watch = Arc::clone(&watch);
    let view = watch.check(true).await;
    let _ = app.emit(EVENT, &view);
    Ok(view)
}

#[tauri::command]
pub fn upstream_watch_mark_seen(app: AppHandle, watch: Watch) -> WatchView {
    let view = watch.mark_seen();
    let _ = app.emit(EVENT, &view);
    view
}

/// Opens a watched upstream's GitHub page in the default browser.
#[tauri::command]
pub fn upstream_open(watch: Watch, url: String) -> Result<(), String> {
    if !watch.may_open(&url) {
        return Err("only a watched upstream's GitHub pages can be opened".into());
    }
    open_in_browser(&url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    // No shell: the URL is one argument to the OS's own opener.
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open the browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn state_is_kept_in_the_given_folder_only() {
        let dir = tempfile::tempdir().unwrap();
        let watch = UpstreamWatch::new(dir.path(), "http://127.0.0.1:9");
        assert_eq!(watch.view().watched.len(), 1);
        watch.mark_seen();
        assert!(dir.path().join("upstream-watch.json").exists());
    }

    #[test]
    fn only_watched_github_pages_may_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        let watch = UpstreamWatch::new(dir.path(), "http://127.0.0.1:9");
        assert!(watch.may_open("https://github.com/Louis-CFM/coucou/commit/abc"));
        assert!(!watch.may_open("https://github.com/Louis-CFM/coucou-evil/x"));
        assert!(!watch.may_open("https://github.com/Louis-CFM/coucou"));
        assert!(!watch.may_open("https://example.com/Louis-CFM/coucou/"));
        assert!(!watch.may_open("https://github.com/Louis-CFM/coucou/ x"));
        assert!(!watch.may_open("file:///etc/passwd"));
    }

    #[test]
    fn a_rate_limit_is_remembered_and_blocks_check_now() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3600"))
                .expect(1)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let watch = UpstreamWatch::new(dir.path(), &server.uri());
            let view = watch.check(true).await;
            assert!(view.rate_limited_until.unwrap() > now());
            assert_eq!(view.last_check, None);
            // A second "Check now" does not reach GitHub (expect(1) above).
            watch.check(true).await;
            // And the limit survives a restart.
            let again = UpstreamWatch::new(dir.path(), &server.uri());
            assert!(again.view().rate_limited_until.is_some());
        });
    }

    #[test]
    fn a_daily_check_waits_a_day_after_the_last() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let watch = UpstreamWatch::new(dir.path(), &server.uri());
            let first = watch.check(false).await;
            assert!(first.last_check.is_some());
            assert!(first.reports[0].error.is_some());
            let hits = server.received_requests().await.unwrap().len();
            watch.check(false).await;
            assert_eq!(server.received_requests().await.unwrap().len(), hits);
        });
    }
}
