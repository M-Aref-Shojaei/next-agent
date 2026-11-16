//! Next Agent's local diagnostics log (ADR-0013, docs/architecture/Logging.md).
//!
//! It records how the app behaves, never what people work on. Events from
//! plain `tracing` macros anywhere in the app become JSON lines under
//! `<app data>/logs/`, keeping only allowlisted fields ([`fields`]) and
//! redacting every string ([`redact()`]). Nothing is ever sent anywhere.
//!
//! This is the only crate that writes under the logs folder
//! (`scripts/check-boundaries.mjs`); other crates just use `tracing`.

pub mod fields;
mod layer;
mod redact;
mod time;
mod writer;

use layer::is_event_name;
pub use redact::redact;
pub use tracing::Level;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;
use tracing_subscriber::layer::SubscriberExt;
use writer::Roller;

/// Events kept in memory for the Diagnostics page.
pub const RING_CAPACITY: usize = 2000;
/// The logs folder's name inside the app data folder.
const LOGS_DIR: &str = "logs";

/// The logs folder for an app data folder: `<app data>/logs`.
#[must_use]
pub fn logs_dir(app_data: &Path) -> PathBuf {
    app_data.join(LOGS_DIR)
}

/// A file name for an export made now:
/// `next-agent-diagnostics-2026-11-15T163401Z.jsonl` (UTC).
#[must_use]
pub fn export_file_name() -> String {
    let (ts, _) = time::stamp(SystemTime::now());
    let compact: String = ts[..19].chars().filter(|c| *c != ':').collect();
    format!("next-agent-diagnostics-{compact}Z.jsonl")
}

/// How to start the log.
#[derive(Debug, Clone)]
pub struct Config {
    /// The logs folder (see [`logs_dir`]).
    pub dir: PathBuf,
    /// The least severe level written.
    pub level: Level,
    /// Files older than this are deleted.
    pub retention_days: u32,
    /// The oldest files are deleted while the folder is larger than this.
    pub max_total_bytes: u64,
    /// Also print each line to stderr (development builds).
    pub stderr: bool,
}

impl Config {
    /// The defaults from the plan: `info`, 14 days, 100 MB, no stderr.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            level: Level::INFO,
            retention_days: 14,
            max_total_bytes: 100 * 1024 * 1024,
            stderr: false,
        }
    }
}

/// One log line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: String,
    pub level: String,
    pub target: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub fields: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub span: Map<String, Value>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped_fields: u32,
}

impl LogEntry {
    /// An entry with no message, fields or span; `emit` stamps `ts`.
    pub(crate) fn new(level: Level, target: &str, event: String) -> Self {
        Self {
            ts: String::new(),
            level: level_name(level).into(),
            target: target.into(),
            event,
            message: None,
            fields: Map::new(),
            span: Map::new(),
            dropped_fields: 0,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if signature.
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Which entries [`Handle::recent`] returns.
#[derive(Debug, Clone, Default)]
pub struct RecentFilter {
    /// Only entries at this level or more severe.
    pub level: Option<Level>,
    /// Only entries of this area.
    pub target: Option<String>,
    /// At most this many (default and cap: [`RING_CAPACITY`]).
    pub limit: Option<usize>,
}

/// What [`Handle::export`] wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportSummary {
    pub path: PathBuf,
    pub entries: usize,
}

/// Why [`init`] failed.
#[derive(Debug)]
pub enum InitError {
    /// The logs folder or file could not be created.
    Io(io::Error),
    /// Another global subscriber is already installed.
    AlreadyInstalled,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not open the logs folder: {error}"),
            Self::AlreadyInstalled => f.write_str("a tracing subscriber is already installed"),
        }
    }
}

impl std::error::Error for InitError {}

/// Levels from most to least severe, with the name the log writes (trace
/// is written as debug; the app does not offer it).
const LEVELS: [(Level, &str); 5] = [
    (Level::ERROR, "error"),
    (Level::WARN, "warn"),
    (Level::INFO, "info"),
    (Level::DEBUG, "debug"),
    (Level::TRACE, "debug"),
];

/// Severity rank: error 1, warn 2, info 3, debug 4, trace 5.
pub(crate) fn rank(level: Level) -> u8 {
    let index = LEVELS.iter().position(|(l, _)| *l == level).unwrap_or(4);
    u8::try_from(index + 1).unwrap_or(5)
}

fn from_rank(rank: u8) -> (Level, &'static str) {
    LEVELS[usize::from(rank.clamp(1, 5)) - 1]
}

pub(crate) fn level_name(level: Level) -> &'static str {
    from_rank(rank(level)).1
}

/// `error`, `warn`, `info` or `debug` (the levels the app offers).
#[must_use]
pub fn parse_level(name: &str) -> Option<Level> {
    LEVELS[..4]
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(l, _)| *l)
}

thread_local! {
    /// Set while this thread is inside the log, so a panic or event raised
    /// from within it cannot re-enter (and deadlock on) the writer.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

pub(crate) struct Inner {
    dir: PathBuf,
    level: AtomicU8,
    stderr: bool,
    roller: Mutex<Roller>,
    ring: Mutex<VecDeque<LogEntry>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    pub(crate) fn enabled(&self, level: Level, target: &str) -> bool {
        let level = rank(level);
        level <= self.level.load(Ordering::Relaxed) && (level <= 2 || layer::is_ours(target))
    }

    /// Stamp, write and remember one entry.
    pub(crate) fn emit(&self, mut entry: LogEntry) {
        /// Clears `BUSY` on every exit, unwinding included, so a panic
        /// inside the log does not silence this thread for good.
        struct Busy;
        impl Drop for Busy {
            fn drop(&mut self) {
                BUSY.with(|b| b.set(false));
            }
        }
        if BUSY.with(|b| b.replace(true)) {
            return;
        }
        let _busy = Busy;
        let (ts, date) = time::stamp(SystemTime::now());
        entry.ts = ts;
        if let Ok(line) = serde_json::to_string(&entry) {
            if self.stderr {
                eprintln!("{line}");
            }
            let _ = lock(&self.roller).write_line(&line, &date);
        }
        let mut ring = lock(&self.ring);
        if ring.len() == RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(entry);
    }

    fn flush(&self) {
        if !BUSY.with(Cell::get) {
            lock(&self.roller).flush();
        }
    }
}

/// The running log. Cheap to clone.
#[derive(Clone)]
pub struct Handle {
    inner: Arc<Inner>,
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("dir", &self.inner.dir)
            .finish_non_exhaustive()
    }
}

/// Build the subscriber without installing it (tests use it scoped).
///
/// # Errors
/// [`InitError::Io`] when the logs folder cannot be created.
fn build(config: Config) -> Result<(impl tracing::Subscriber + Send + Sync, Handle), InitError> {
    let roller = Roller::open(&config.dir, config.retention_days, config.max_total_bytes)
        .map_err(InitError::Io)?;
    let inner = Arc::new(Inner {
        dir: config.dir,
        level: AtomicU8::new(rank(config.level)),
        stderr: config.stderr,
        roller: Mutex::new(roller),
        ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
    });
    let subscriber = tracing_subscriber::registry().with(layer::DiagLayer {
        inner: Arc::clone(&inner),
    });
    Ok((subscriber, Handle { inner }))
}

/// Install the global subscriber and a panic hook that logs the panic and
/// flushes the file. Call once, first thing at startup.
///
/// # Errors
/// [`InitError`] when the folder cannot be created or a subscriber is
/// already installed.
pub fn init(config: Config) -> Result<Handle, InitError> {
    let (subscriber, handle) = build(config)?;
    tracing::subscriber::set_global_default(subscriber).map_err(|_| InitError::AlreadyInstalled)?;
    let inner = Arc::clone(&handle.inner);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic(&inner, info);
        previous(info);
    }));
    Ok(handle)
}

/// A panic's source file: our own repo-relative sources (`crates/core/src/
/// store.rs`) are kept as they are; anything else (a dependency under the
/// home folder) is redacted.
fn panic_file(file: &str) -> String {
    let ours = (file.starts_with("crates/") || file.starts_with("apps/"))
        && file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b));
    if ours {
        file.to_string()
    } else {
        redact(file)
    }
}

fn log_panic(inner: &Inner, info: &std::panic::PanicHookInfo<'_>) {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let mut fields = Map::new();
    if let Some(location) = info.location() {
        let file = location.file();
        let krate = file
            .strip_prefix("crates/")
            .or_else(|| file.strip_prefix("apps/"))
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("external");
        fields.insert("crate".into(), krate.into());
        fields.insert("file".into(), panic_file(file).into());
        fields.insert("line".into(), location.line().into());
    }
    inner.emit(LogEntry {
        message: Some(redact(&payload)).filter(|m| !m.is_empty()),
        fields,
        ..LogEntry::new(Level::ERROR, "app", "panic".into())
    });
    inner.flush();
}

impl Handle {
    /// Change the least severe level written, at once.
    pub fn set_level(&self, level: Level) {
        self.inner.level.store(rank(level), Ordering::Relaxed);
    }

    /// The current level.
    #[must_use]
    pub fn level(&self) -> Level {
        from_rank(self.inner.level.load(Ordering::Relaxed)).0
    }

    /// Recent entries from memory, newest first.
    #[must_use]
    pub fn recent(&self, filter: &RecentFilter) -> Vec<LogEntry> {
        let limit = filter.limit.unwrap_or(RING_CAPACITY).min(RING_CAPACITY);
        let max = filter.level.map_or(u8::MAX, rank);
        lock(&self.inner.ring)
            .iter()
            .rev()
            .filter(|e| parse_level(&e.level).is_some_and(|l| rank(l) <= max))
            .filter(|e| filter.target.as_ref().is_none_or(|t| *t == e.target))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Log an event from outside `tracing` (the UI's `diag_log`), through the
    /// same allowlist and redaction, with target `ui`. A string `message`
    /// field becomes the entry's (redacted) message. Returns whether it was
    /// written (the level may filter it out).
    ///
    /// # Errors
    /// When `event` is not a `snake_case` name.
    pub fn record(
        &self,
        level: Level,
        event: &str,
        fields: &Map<String, Value>,
    ) -> Result<bool, String> {
        if !is_event_name(event) {
            return Err("event must be a snake_case name of at most 64 characters".into());
        }
        if !self.inner.enabled(level, "ui") {
            return Ok(false);
        }
        let mut collect = layer::Collect::default();
        for (name, value) in fields {
            collect.put(name, value.clone());
        }
        // The event name is the argument; an `event` field is dropped.
        let dropped = collect.dropped + u32::from(collect.event.is_some());
        self.inner.emit(LogEntry {
            // A UI error's text (`ui_error`): free text, redacted.
            message: collect
                .message
                .filter(|m| !m.is_empty())
                .map(|m| redact(&m)),
            fields: collect.fields,
            dropped_fields: dropped,
            ..LogEntry::new(level, "ui", event.into())
        });
        Ok(true)
    }

    /// Write every log file, oldest first, to `to` as one `.jsonl` file with
    /// a system-info header line. Each line is re-checked against the
    /// allowlist and redacted again on the way out.
    ///
    /// # Errors
    /// A message when `to` cannot be written.
    pub fn export(&self, to: &Path) -> Result<ExportSummary, String> {
        self.inner.flush();
        let mut out = writer::open_private(to)
            .and_then(|f| f.set_len(0).map(|()| f))
            .map_err(|e| format!("could not write the export: {e}"))?;
        let (ts, _) = time::stamp(SystemTime::now());
        let header = serde_json::json!({
            "export": "next-agent-diagnostics",
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "level": level_name(self.level()),
            "exported_at": ts,
        });
        let mut entries = 0;
        let mut write = |line: &str| {
            writeln!(out, "{line}").map_err(|e| format!("could not write the export: {e}"))
        };
        write(&header.to_string())?;
        for (path, _, _) in writer::log_files(&self.inner.dir) {
            let Ok(file) = File::open(&path) else {
                continue;
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let Ok(entry) = serde_json::from_str::<LogEntry>(&line) else {
                    continue;
                };
                if let Ok(clean) = serde_json::to_string(&resanitize(entry)) {
                    write(&clean)?;
                    entries += 1;
                }
            }
        }
        Ok(ExportSummary {
            path: to.to_path_buf(),
            entries,
        })
    }

    /// The logs folder.
    #[must_use]
    pub fn folder(&self) -> &Path {
        &self.inner.dir
    }
}

/// Apply the allowlist and redaction again to an entry read back from disk.
fn resanitize(entry: LogEntry) -> LogEntry {
    let panic = entry.event == "panic";
    let mut fields = layer::Collect::default();
    for (name, value) in entry.fields {
        // The panic hook's own fields (crate, file, line) are not tracing fields.
        if panic && matches!(name.as_str(), "crate" | "file" | "line") {
            let value = match value {
                Value::String(s) if name == "file" => Value::String(panic_file(&s)),
                Value::String(s) => Value::String(redact(&s)),
                other => other,
            };
            fields.fields.insert(name, value);
        } else {
            fields.put(&name, value);
        }
    }
    let mut span = layer::Collect::default();
    for (name, value) in entry.span {
        span.put(&name, value);
    }
    LogEntry {
        level: parse_level(&entry.level).map_or("debug", level_name).into(),
        target: layer::area(&entry.target).into(),
        event: if is_event_name(&entry.event) {
            entry.event
        } else {
            "unnamed".into()
        },
        message: entry.message.map(|m| redact(&m)),
        fields: fields.fields,
        span: span.fields,
        dropped_fields: entry.dropped_fields + fields.dropped + span.dropped,
        ts: entry.ts,
    }
}

#[cfg(test)]
mod tests;
