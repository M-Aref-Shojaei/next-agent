//! End-to-end tests of the pipeline: tracing macro → layer → file and ring.
//! The canary tests are the proof of ADR-0013's privacy rules.

use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};

const PROMPT: &str = "CANARY-PROMPT-7f3a please rewrite my secret business plan";
const PROMPT_MARK: &str = "CANARY-PROMPT-7f3a";
const SK_ANT: &str = "sk-ant-api03-CANARYkey0123456789abcdefghijklmnopqrstuv";
const BOT: &str = "7012345678:AAHcanaryBotToken_abcdefghijklmnopqrstu";
const EMAIL: &str = "canary.person@example.com";
const HOME_PATH: &str = "/Users/canary-user/Documents/canary-notes/plan.md";
const NOTE: &str = "CANARY-NOTE-91be my diary entry";

/// Every canary and the parts of them that must not survive.
const NEEDLES: &[&str] = &[
    PROMPT_MARK,
    "secret business plan",
    "CANARYkey",
    "AAHcanaryBotToken",
    "7012345678",
    EMAIL,
    "canary.person",
    "canary-user",
    "canary-notes",
    "CANARY-NOTE",
    "diary entry",
];

fn start(
    level: Level,
) -> (
    tempfile::TempDir,
    impl tracing::Subscriber + Send + Sync,
    Handle,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::new(logs_dir(dir.path()));
    config.level = level;
    let (subscriber, handle) = build(config).unwrap();
    (dir, subscriber, handle)
}

fn file_text(handle: &Handle) -> String {
    writer::log_files(handle.folder())
        .into_iter()
        .map(|(path, _, _)| fs_read(&path))
        .collect()
}

fn fs_read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn lines(handle: &Handle) -> Vec<Value> {
    file_text(handle)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn assert_clean(where_: &str, text: &str) {
    for needle in NEEDLES {
        assert!(
            !text.contains(needle),
            "{needle:?} reached the {where_}:\n{text}"
        );
    }
}

#[test]
fn canary_disallowed_fields_never_reach_the_log() {
    let (_dir, subscriber, handle) = start(Level::DEBUG);
    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!(
            target: "router",
            prompt = PROMPT,
            content = %NOTE,
            text = ?PROMPT,
            body = PROMPT,
            answer = PROMPT,
            response = PROMPT,
            title = NOTE,
            name = NOTE,
            note = NOTE,
            query = PROMPT,
            args = PROMPT,
            input = PROMPT,
            output = PROMPT,
            diff = NOTE,
            path = HOME_PATH,
            token = BOT,
            key = SK_ANT,
            secret = SK_ANT,
            password = SK_ANT,
            authorization = SK_ANT,
            sender = EMAIL,
            username = EMAIL,
            email = EMAIL,
            phone = "+44 20 7946 0958",
            "provider_call"
        );
        let span = tracing::info_span!(
            "task",
            task_id = "t_01CANARY",
            prompt = PROMPT,
            title = NOTE
        );
        let _guard = span.enter();
        tracing::info!(target: "tasks", mode = "auto", "task_started");
    });
    let text = file_text(&handle);
    assert_clean("log file", &text);
    assert_clean(
        "ring buffer",
        &serde_json::to_string(&handle.recent(&RecentFilter::default())).unwrap(),
    );
    let all = lines(&handle);
    assert_eq!(all[0]["event"], "provider_call");
    assert_eq!(all[0]["dropped_fields"], 24);
    assert_eq!(all[0]["fields"], json!({}));
    assert_eq!(all[1]["span"], json!({ "task_id": "t_01CANARY" }));
    assert_eq!(all[1]["fields"], json!({ "mode": "auto" }));
}

#[test]
fn canary_secrets_in_allowed_fields_are_redacted() {
    let (_dir, subscriber, handle) = start(Level::DEBUG);
    tracing::subscriber::with_default(subscriber, || {
        // Free-text fields: shapes are redacted.
        tracing::debug!(
            target: "channels",
            error = %format!("send failed for {EMAIL} with {SK_ANT} via https://api.telegram.org/bot{BOT}/sendMessage at {HOME_PATH}"),
            reason = BOT,
            "api_error"
        );
        // Token-shaped fields: free text is refused outright, secrets redacted.
        tracing::debug!(
            target: "router",
            model = PROMPT,
            provider = SK_ANT,
            channel_id = BOT,
            session_id = EMAIL,
            page = HOME_PATH,
            command = NOTE,
            "provider_call_failed"
        );
        // A free-text message.
        tracing::warn!(target: "router", "call for {EMAIL} failed: Bearer {SK_ANT} in {HOME_PATH}");
        // An error value through record_error.
        let error: Box<dyn std::error::Error> = format!("token {BOT} rejected").into();
        tracing::error!(target: "channels", error = error.as_ref() as &dyn std::error::Error, "api_error");
    });
    let text = file_text(&handle);
    assert_clean("log file", &text);
    assert_clean(
        "ring buffer",
        &serde_json::to_string(&handle.recent(&RecentFilter::default())).unwrap(),
    );
    let all = lines(&handle);
    assert_eq!(all[1]["fields"]["model"], fields::INVALID);
    assert_eq!(all[1]["fields"]["command"], fields::INVALID);
    assert_eq!(all[1]["fields"]["provider"], "[secret]");
    assert_eq!(all[1]["fields"]["channel_id"], "[secret]");
    assert_eq!(all[1]["fields"]["session_id"], "[redacted]");
    assert_eq!(all[1]["fields"]["page"], "~/…/*.md (depth 3)");
    assert_eq!(all[2]["event"], "unnamed");
}

#[test]
fn canary_ui_events_go_through_the_same_rules() {
    let (_dir, _subscriber, handle) = start(Level::DEBUG);
    let fields = json!({
        "prompt": PROMPT,
        "message": format!("TypeError at {HOME_PATH} for {EMAIL}"),
        "event": "fake",
        "error": format!("{SK_ANT} {EMAIL} {HOME_PATH}"),
        "page": PROMPT,
        "window": { "title": NOTE },
        "count": [1, 2],
        "duration_ms": 812,
        "ok": false,
    });
    let Value::Object(fields) = fields else {
        unreachable!()
    };
    assert_eq!(handle.record(Level::DEBUG, "ui_error", &fields), Ok(true));
    assert!(handle
        .record(Level::INFO, "Bad Event", &Map::new())
        .is_err());
    let text = file_text(&handle);
    assert_clean("log file", &text);
    let entry = &lines(&handle)[0];
    assert_eq!(entry["target"], "ui");
    assert_eq!(entry["event"], "ui_error");
    assert_eq!(entry["fields"]["duration_ms"], 812);
    assert_eq!(entry["fields"]["ok"], false);
    assert_eq!(entry["fields"]["page"], fields::INVALID);
    assert_eq!(
        entry["message"],
        "TypeError at ~/…/*.md (depth 3) for [redacted]"
    );
    // prompt, event, window (object) and count (array).
    assert_eq!(entry["dropped_fields"], 4);
}

#[test]
fn canary_export_redacts_again() {
    let (_dir, subscriber, handle) = start(Level::INFO);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "app", version = "0.1.0", os = "macos", arch = "aarch64", "app_started");
    });
    // A line written by an older or buggy build, straight to the file.
    let (_, date) = time::stamp(SystemTime::now());
    let raw = json!({
        "ts": "2026-11-15T00:00:00.000Z", "level": "warn", "target": "router", "event": "x",
        "message": format!("leak {SK_ANT} {EMAIL}"),
        "fields": { "prompt": PROMPT, "error": HOME_PATH, "model": PROMPT },
        "span": { "title": NOTE, "task_id": "t_1" },
    });
    let file = handle.folder().join(format!("next-agent-{date}.jsonl"));
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    writeln!(f, "{raw}").unwrap();
    writeln!(f, "not json at all {PROMPT}").unwrap();

    let out = tempfile::tempdir().unwrap();
    let to = out.path().join("export.jsonl");
    let summary = handle.export(&to).unwrap();
    assert_eq!(summary.entries, 2);
    let text = fs_read(&to);
    assert_clean("export", &text);
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["export"], "next-agent-diagnostics");
    assert_eq!(lines[1]["event"], "app_started");
    assert_eq!(lines[2]["span"], json!({ "task_id": "t_1" }));
    assert_eq!(lines[2]["dropped_fields"], 2);
}

#[test]
fn event_shape_matches_the_plan() {
    let (_dir, subscriber, handle) = start(Level::INFO);
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("task", task_id = "t_01J");
        let _g = span.enter();
        tracing::warn!(
            target: "router",
            provider = "anthropic",
            model = "claude-sonnet-4-5",
            status = 529,
            duration_ms = 812_u64,
            attempt = 2,
            "provider_call_failed"
        );
    });
    // Keys in the plan's order (serde_json::Value sorts them, so check the text).
    let text = file_text(&handle);
    let order: Vec<usize> = [
        "\"ts\":",
        "\"level\":",
        "\"target\":",
        "\"event\":",
        "\"fields\":",
        "\"span\":",
    ]
    .iter()
    .map(|k| text.find(k).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
    let entry = &lines(&handle)[0];
    assert_eq!(entry.as_object().unwrap().len(), 6);
    assert_eq!(entry["level"], "warn");
    assert_eq!(entry["target"], "router");
    assert_eq!(
        entry["fields"],
        json!({"provider":"anthropic","model":"claude-sonnet-4-5","status":529,"duration_ms":812,"attempt":2})
    );
    assert_eq!(entry["span"], json!({ "task_id": "t_01J" }));
    let ts = entry["ts"].as_str().unwrap();
    assert_eq!(ts.len(), 24);
    assert!(ts.ends_with('Z'));
}

#[test]
fn level_changes_at_runtime() {
    let (_dir, subscriber, handle) = start(Level::WARN);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "app", "hidden_one");
        handle.set_level(Level::DEBUG);
        tracing::debug!(target: "app", "shown_debug");
        handle.set_level(Level::ERROR);
        tracing::warn!(target: "app", "hidden_two");
        tracing::error!(target: "app", "shown_error");
    });
    let events: Vec<String> = lines(&handle)
        .iter()
        .map(|e| e["event"].as_str().unwrap().into())
        .collect();
    assert_eq!(events, ["shown_debug", "shown_error"]);
    assert_eq!(handle.level(), Level::ERROR);
}

#[test]
fn other_crates_are_kept_only_at_warn_and_above() {
    let (_dir, subscriber, handle) = start(Level::DEBUG);
    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!(target: "hyper_util::client::legacy", "connecting");
        tracing::info!(target: "tauri::manager", "started");
        tracing::warn!(target: "hyper_util::client::legacy", "connection reset");
        tracing::debug!(target: "next_agent_core::broker", "ours");
    });
    let targets: Vec<(String, String)> = lines(&handle)
        .iter()
        .map(|e| {
            (
                e["target"].as_str().unwrap().into(),
                e["event"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        [
            ("app".into(), "unnamed".into()),
            ("broker".into(), "ours".into())
        ]
    );
}

#[test]
fn ring_buffer_keeps_the_newest_2000_and_filters() {
    let (_dir, subscriber, handle) = start(Level::DEBUG);
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..2005_u64 {
            tracing::info!(target: "tasks", count = i, "tick");
        }
        tracing::error!(target: "router", "breaker_opened");
    });
    let all = handle.recent(&RecentFilter::default());
    assert_eq!(all.len(), RING_CAPACITY);
    assert_eq!(all[0].event, "breaker_opened");
    assert_eq!(all[1].fields["count"], 2004);
    assert_eq!(all.last().unwrap().fields["count"], 6);
    let errors = handle.recent(&RecentFilter {
        level: Some(Level::WARN),
        ..RecentFilter::default()
    });
    assert_eq!(errors.len(), 1);
    let tasks = handle.recent(&RecentFilter {
        target: Some("tasks".into()),
        limit: Some(3),
        ..RecentFilter::default()
    });
    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks[0].fields["count"], 2004);
}

#[test]
fn existing_style_calls_do_not_panic_and_count_dropped_fields() {
    // The shapes used by crates/core and crates/channels today.
    let (_dir, subscriber, handle) = start(Level::DEBUG);
    tracing::subscriber::with_default(subscriber, || {
        let error = std::io::Error::other("disk full");
        let seq = 4_u64;
        tracing::error!(seq, kind = "task.started", problem = %error, "event sink failed");
        tracing::warn!(%error, call_id = "c1", "step snapshot failed");
        tracing::warn!(target: "router", budget = %"daily", spent = 1.5_f64, "budget above {:.0}%", 80.0);
        tracing::info!(target: "http_fetch", status = 200_u16, bytes = 12_usize, "http_fetch completed");
    });
    let all = lines(&handle);
    assert_eq!(all.len(), 4);
    assert_eq!(all[0]["dropped_fields"], 2);
    assert_eq!(all[0]["target"], "app");
    assert_eq!(all[0]["message"], "event sink failed");
    assert_eq!(all[1]["fields"]["error"], "disk full");
    assert_eq!(all[2]["message"], "budget above 80%");
}

#[test]
fn panics_are_logged_redacted_and_flushed() {
    let (_dir, _subscriber, handle) = start(Level::INFO);
    let inner = Arc::clone(&handle.inner);
    let seen = Arc::new(StdMutex::new(false));
    let seen_hook = Arc::clone(&seen);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic(&inner, info);
        *seen_hook.lock().unwrap() = true;
    }));
    let result = std::panic::catch_unwind(|| -> u8 { panic!("boom for {EMAIL} with {SK_ANT}") });
    std::panic::set_hook(previous);
    assert!(result.is_err());
    assert!(*seen.lock().unwrap());
    let text = file_text(&handle);
    assert_clean("log file", &text);
    // Other tests' panics on other threads may land here too while the hook is set.
    let entry = lines(&handle)
        .into_iter()
        .find(|e| e["message"].as_str().is_some_and(|m| m.starts_with("boom")))
        .unwrap();
    assert_eq!(entry["event"], "panic");
    assert_eq!(entry["level"], "error");
    assert_eq!(entry["fields"]["crate"], "diagnostics");
    assert!(entry["fields"]["line"].as_u64().unwrap() > 0);
    assert!(entry["message"]
        .as_str()
        .unwrap()
        .starts_with("boom for [redacted]"));
}

#[test]
fn init_installs_once() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::new(logs_dir(dir.path()));
    // The first call may succeed or not (another test binary state); the
    // second must report that a subscriber is already installed.
    let _ = init(config.clone());
    assert!(matches!(init(config), Err(InitError::AlreadyInstalled)));
}

#[test]
fn export_file_names_are_dated() {
    let name = export_file_name();
    assert!(name.starts_with("next-agent-diagnostics-20"), "{name}");
    assert!(name.ends_with("Z.jsonl"), "{name}");
    assert!(!name.contains(':'), "{name}");
}

#[test]
fn levels_parse() {
    assert_eq!(parse_level("debug"), Some(Level::DEBUG));
    assert_eq!(parse_level("warn"), Some(Level::WARN));
    assert_eq!(parse_level("trace"), None);
    assert_eq!(parse_level("INFO"), None);
}
