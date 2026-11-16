//! End-to-end canary tests of the diagnostics log (ADR-0013,
//! docs/architecture/Logging.md, "Audit 2026-11-16").
//!
//! They install the real `next_agent_diagnostics` subscriber at `debug`,
//! writing to a temp folder, then drive the app's own paths with canary
//! strings: a task through the gateway and a mock provider (tool calls that
//! write a file, save a memory and create a skill), a provider error that
//! echoes the prompt and the key, a channel message through a mock Telegram,
//! a notes page save, a board save, the memory editor and a cron run. Each
//! test then reads every log file and the ring buffer and asserts that no
//! canary and no secret shape is there, and that the expected events are.
//!
//! They live in the crate (not `tests/`) because only a test build swaps the
//! OS keychain for memory (`gateway::keys`). The subscriber is global, so the
//! other unit tests of this binary log into the same folder once it is
//! installed; that only widens what the "no canary" check covers. Expected
//! events are matched on this test's own task, channel or cron id where the
//! event has one.

use crate::boards;
use crate::channels::{ChannelInput, ChannelService};
use crate::gateway::ConnectionInput;
use crate::learning::{CronInput, LearningSettings};
use crate::notes::NotesService;
use crate::service::{AppService, Sink, TaskRequest};
use crate::wire::StreamMessage;
use next_agent_channels::{ChannelConfig, ChannelMode};
use next_agent_core::learning::{cron, sessions};
use next_agent_core::router::{GatewaySettings, MemorySecret, SecretStore};
use next_agent_design::proxy::PreviewProxy;
use next_agent_diagnostics::{self as diag, Config, Handle, Level, RecentFilter};
use next_agent_graph::BlockNode;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROMPT_MARK: &str = "CANARY-PROMPT-7f3a";
const SK_ANT: &str = "sk-ant-api03-CANARYkey0123456789abcdefghijklmnopqrstuvwxyzAB";
/// `\d+:` and 35 characters, the Telegram bot token shape.
const BOT_TOKEN: &str = "123456789:AAcanaryBotToken0123456789abcdefghi";
const EMAIL: &str = "canary.person@example.com";
const PAGE: &str = "Canary Secret Diary 7f3a";
const BOARD: &str = "Canary Board 7f3a";
const SKILL: &str = "canary-skill-7f3a";
const FILE: &str = "canary-file-7f3a.md";
const HOME_PATH: &str = "/Users/canary-user/Documents/canary-notes/plan.md";
/// The workspace folder: every workspace path carries it.
const WORKSPACE: &str = "canary-ws-7f3a";
const SENDER: &str = "777000123";
const STRANGER: &str = "777000124";

/// Every canary, or the part of it that must not survive redaction.
const NEEDLES: &[&str] = &[
    PROMPT_MARK,
    "CANARYkey",
    "AAcanaryBotToken",
    "123456789:",
    "canary.person",
    "Canary Secret Diary",
    "Canary Board",
    "canary-skill",
    "canary-file",
    "canary-user",
    "canary-notes",
    WORKSPACE,
    SENDER,
    STRANGER,
    "canary_person",
    "canary_stranger",
];

/// A prompt, message or note that carries every canary at once.
fn canary_text() -> String {
    format!(
        "{PROMPT_MARK} rewrite my plan. Key {SK_ANT}, bot {BOT_TOKEN}, mail {EMAIL}, \
         file {HOME_PATH}, page [[{PAGE}]], skill {SKILL}, board {BOARD}"
    )
}

/// Secret shapes that must never be written, whatever test produced them.
fn secret_shapes() -> &'static [Regex] {
    static SHAPES: OnceLock<Vec<Regex>> = OnceLock::new();
    SHAPES.get_or_init(|| {
        [
            r"sk-ant-",
            r"\bsk-[A-Za-z0-9_-]{16,}",
            r"\b\d{6,}:[A-Za-z0-9_-]{30,}",
            r"[\w.+-]+@[\w-]+\.[\w.-]+",
            r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("regex"))
        .collect()
    })
}

/// The one global subscriber of this test binary, at `debug`, in a temp
/// folder that lives as long as the process.
fn log() -> &'static Handle {
    static LOG: OnceLock<(tempfile::TempDir, Handle)> = OnceLock::new();
    &LOG.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tmp");
        let mut config = Config::new(diag::logs_dir(dir.path()));
        config.level = Level::DEBUG;
        let handle = diag::init(config).expect("the canary tests install the only subscriber");
        (dir, handle)
    })
    .1
}

/// Every log file's text, oldest first.
fn file_text(handle: &Handle) -> String {
    let mut files: Vec<PathBuf> = std::fs::read_dir(handle.folder())
        .expect("logs folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    files
        .iter()
        .map(|f| std::fs::read_to_string(f).expect("log file"))
        .collect()
}

/// Every entry in the files.
fn entries(handle: &Handle) -> Vec<Value> {
    file_text(handle)
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

/// No canary and no secret shape in any log file or in the ring buffer.
fn assert_clean(handle: &Handle) {
    let ring = serde_json::to_string(&handle.recent(&RecentFilter::default())).expect("json");
    for (place, text) in [("log files", file_text(handle)), ("ring buffer", ring)] {
        for needle in NEEDLES {
            let hit = text.lines().find(|l| l.contains(needle));
            assert!(hit.is_none(), "{needle:?} reached the {place}: {hit:?}");
        }
        for shape in secret_shapes() {
            let hit = text.lines().find(|l| shape.is_match(l));
            assert!(hit.is_none(), "{shape} matched in the {place}: {hit:?}");
        }
    }
}

/// Whether an entry belongs to `id` (in its fields or its span).
fn of(entry: &Value, key: &str, id: &str) -> bool {
    entry["fields"][key] == id || entry["span"][key] == id
}

/// The names of the events that match `pred`.
fn events_where(handle: &Handle, pred: impl Fn(&Value) -> bool) -> Vec<String> {
    entries(handle)
        .iter()
        .filter(|e| pred(e))
        .filter_map(|e| e["event"].as_str().map(str::to_string))
        .collect()
}

fn assert_has(events: &[String], expected: &[&str]) {
    for name in expected {
        assert!(
            events.iter().any(|e| e == name),
            "no {name} event; got {events:?}"
        );
    }
}

/// Poll the log until `pred` holds for some set of events, or fail.
fn wait_for(handle: &Handle, what: &str, pred: impl Fn(&[Value]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !pred(&entries(handle)) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// An app service and a workspace folder named after a canary.
fn service(dir: &Path) -> (Arc<AppService>, String) {
    let svc = Arc::new(AppService::new(dir.join("db.sqlite")).expect("service"));
    let ws = dir.join(WORKSPACE);
    std::fs::create_dir_all(&ws).expect("workspace");
    let id = svc
        .workspace_open(ws.to_str().expect("utf8"))
        .expect("open")
        .id;
    // No background review: the provider mock answers the task only.
    svc.learning_settings_set(&LearningSettings {
        background_review: false,
    })
    .expect("settings");
    (svc, id)
}

/// A sink that allows every approval (as the user would) and keeps nothing.
fn allowing_sink(svc: &Arc<AppService>) -> Sink {
    let responder = Arc::clone(svc);
    Arc::new(move |m| {
        if let StreamMessage::Event { event } = &m {
            if event["kind"] == "approval_requested" {
                let task = event["task_id"].as_str().expect("task");
                let approval = event["data"]["approval_id"].as_str().expect("id");
                let _ = responder.approval_respond(task, approval, "allow");
            }
        }
    })
}

/// An OpenAI-compatible stream of `chunks`, with usage.
fn sse(chunks: &[Value]) -> ResponseTemplate {
    let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    body.push_str(
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":42,\"completion_tokens\":3}}\n\n",
    );
    body.push_str("data: [DONE]\n\n");
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

fn tool_call(index: u32, name: &str, args: &Value) -> Value {
    json!({"choices":[{"delta":{"tool_calls":[{
        "index": index, "id": format!("call_{index}"), "type": "function",
        "function": {"name": name, "arguments": args.to_string()}
    }]}}]})
}

/// A custom (OpenAI-compatible) provider at `server` with the canary key,
/// as the chat model.
fn connect(svc: &AppService, server: &MockServer) {
    svc.gateway_connection_save(&ConnectionInput {
        provider: "custom".into(),
        base_url: format!("{}/v1", server.uri()),
        api_key: SK_ANT.into(),
        ..ConnectionInput::default()
    })
    .expect("connection");
    let mut settings = GatewaySettings::default();
    settings
        .roles
        .insert("chat".into(), "custom/canary-model".into());
    svc.gateway_settings_set(&settings).expect("settings");
}

fn start_task(svc: &Arc<AppService>, workspace_id: &str) -> String {
    let request = TaskRequest {
        workspace_id: workspace_id.into(),
        prompt: canary_text(),
        mode: "manual".into(),
        profile: None,
    };
    let task = svc.task_start(&request, allowing_sink(svc)).expect("start");
    svc.task_wait(&task);
    task
}

/// The agent's loop through the gateway: the provider answers with tool
/// calls carrying canaries (write a file, save a memory, create a skill),
/// then with a final answer echoing them.
#[test]
fn canary_task_through_the_gateway_with_tools_stays_out_of_the_log() {
    let log = log();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let server = runtime.block_on(MockServer::start());
    let skill_md = format!(
        "---\nname: {SKILL}\ndescription: Use when {PROMPT_MARK} comes up.\n---\n\
         Mail {EMAIL}. Open [[{PAGE}]] and {HOME_PATH}.\n"
    );
    let calls = [
        json!({"choices":[{"delta":{"role":"assistant","content": canary_text()}}]}),
        tool_call(
            0,
            "write_file",
            &json!({"path": FILE, "contents": canary_text()}),
        ),
        tool_call(
            1,
            "memory",
            &json!({"target": "memory", "operations": [{"action": "add",
                "content": format!("{PROMPT_MARK}: the diary is {PAGE}; mail {EMAIL}")}]}),
        ),
        tool_call(
            2,
            "skill_manage",
            &json!({"operations": [{"name": SKILL, "action": "create", "content": skill_md}]}),
        ),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let done = [
        json!({"choices":[{"delta":{"content": format!("Done: {}", canary_text())}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
    ];
    runtime.block_on(async {
        // After the tool results: the final answer. First registered wins.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("\"role\":\"tool\""))
            .respond_with(sse(&done))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(sse(&calls))
            .mount(&server)
            .await;
    });
    let dir = tempfile::tempdir().expect("tmp");
    let (svc, ws) = service(dir.path());
    connect(&svc, &server);

    let task = start_task(&svc, &ws);

    let root = dir.path().join(WORKSPACE);
    assert!(root.join(FILE).exists(), "the write ran");
    assert!(
        root.join(".next-agent/skills").join(SKILL).exists(),
        "the skill was created"
    );
    assert_clean(log);
    let mine = events_where(log, |e| of(e, "task_id", &task));
    assert_has(
        &mine,
        &[
            "task_started",
            "provider_call",
            "tool_decision",
            "approval_requested",
            "task_finished",
        ],
    );
    let all = events_where(log, |_| true);
    assert_has(&all, &["memory_saved", "skill_changed"]);
}

/// A provider error whose body echoes the prompt and the key: the task
/// fails, the log keeps the status and the error type only.
#[test]
fn canary_provider_error_echoing_the_prompt_stays_out_of_the_log() {
    let log = log();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "type": "invalid_request_error",
                    "message": format!("bad prompt: {} (key {SK_ANT})", canary_text())
                }
            })))
            .mount(&server),
    );
    let dir = tempfile::tempdir().expect("tmp");
    let (svc, ws) = service(dir.path());
    connect(&svc, &server);

    let task = start_task(&svc, &ws);

    assert_clean(log);
    let failed: Vec<Value> = entries(log)
        .into_iter()
        .filter(|e| of(e, "task_id", &task) && e["event"] == "provider_call_failed")
        .collect();
    assert!(!failed.is_empty(), "no provider_call_failed for {task}");
    assert_eq!(failed[0]["fields"]["status"], 400, "{failed:?}");
    assert_has(
        &events_where(log, |e| of(e, "task_id", &task)),
        &["task_started", "task_finished"],
    );
}

fn telegram_ok(result: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": result }))
}

fn telegram_message(update: u64, from: &str, user: &str, text: &str) -> Value {
    json!({ "update_id": update, "message": {
        "message_id": update, "date": 0, "text": text,
        "from": { "id": from.parse::<u64>().expect("id"), "is_bot": false,
                  "username": user, "first_name": "Canary" },
        "chat": { "id": from.parse::<u64>().expect("id"), "type": "private" }
    }})
}

/// A message from an allowed sender runs a channel task and is answered; a
/// message from a stranger gets a pairing code. Text, sender ids, user
/// names, the pairing code and the bot token stay out of the log.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canary_channel_message_through_mock_telegram_stays_out_of_the_log() {
    let log = log();
    let server = MockServer::start().await;
    let api = |name: &str| format!("/bot{BOT_TOKEN}/{name}");
    Mock::given(method("POST"))
        .and(path(api("getMe")))
        .respond_with(telegram_ok(
            &json!({ "id": 999, "is_bot": true, "username": "NextBot", "first_name": "Next" }),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .and(body_partial_json(json!({ "offset": 0 })))
        .respond_with(telegram_ok(&json!([
            telegram_message(10, SENDER, "canary_person", &canary_text()),
            telegram_message(
                11,
                STRANGER,
                "canary_stranger",
                &format!("{PROMPT_MARK} let me in, {EMAIL}")
            ),
        ])))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .respond_with(telegram_ok(&json!([])).set_delay(Duration::from_millis(300)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("sendMessage")))
        .respond_with(telegram_ok(&json!({
            "message_id": 50, "date": 0, "chat": { "id": 1, "type": "private" }, "text": "ok"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(telegram_ok(&json!(true)))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tmp");
    let (app, ws) = service(dir.path());
    let secrets: Arc<Mutex<HashMap<String, Arc<MemorySecret>>>> = Arc::default();
    let store = Arc::clone(&secrets);
    let channels = ChannelService::with(
        app,
        Arc::new(|_, _| {}),
        Arc::new(move |id: &str| {
            Arc::clone(
                store
                    .lock()
                    .expect("lock")
                    .entry(id.to_string())
                    .or_default(),
            ) as Arc<dyn SecretStore>
        }),
        &server.uri(),
    )
    .expect("channels");
    let mut config = ChannelConfig {
        workspace_id: ws,
        enabled: false,
        mode: ChannelMode::Plan,
        name: format!("{PAGE} bot"),
        ..ChannelConfig::default()
    };
    let saved = channels
        .save(ChannelInput {
            config: config.clone(),
            token: Some(BOT_TOKEN.into()),
        })
        .await
        .expect("save");
    let id = saved.config.id.clone();
    channels
        .allow_add(&id, SENDER, "@canary_person")
        .expect("allow");
    config.id.clone_from(&id);
    config.enabled = true;
    channels
        .save(ChannelInput {
            config,
            token: None,
        })
        .await
        .expect("enable");

    // The pairing reply and the task's answer are both sent.
    let channel = id.clone();
    tokio::task::spawn_blocking(move || {
        wait_for(log, "the pairing and the task answer", |all| {
            let mine = |name: &str| {
                all.iter()
                    .filter(|e| e["event"] == name && of(e, "channel_id", &channel))
                    .count()
            };
            mine("pairing_requested") >= 1 && mine("message_out") >= 2
        });
    })
    .await
    .expect("wait");
    channels.delete(&id).await.expect("delete");

    assert_clean(log);
    assert_has(
        &events_where(log, |e| of(e, "channel_id", &id)),
        &[
            "channel_started",
            "message_in",
            "pairing_requested",
            "message_out",
        ],
    );
}

/// A notes page and a board, named and filled with canaries, saved through
/// the services the UI calls; a file the watcher picks up; a board conflict.
#[test]
fn canary_notes_page_and_board_saves_stay_out_of_the_log() {
    let log = log();
    let dir = tempfile::tempdir().expect("tmp");
    let graph = dir.path().join(WORKSPACE).join("canary-notes");
    std::fs::create_dir_all(&graph).expect("graph");
    let notes = NotesService::new(dir.path().join("cache"));
    notes
        .open(graph.to_str().expect("utf8"), true)
        .expect("open");
    let tree = vec![BlockNode {
        content: canary_text(),
        children: vec![BlockNode {
            content: format!("[[{PAGE}]] {EMAIL} {HOME_PATH}"),
            ..BlockNode::default()
        }],
        ..BlockNode::default()
    }];
    notes.save_page(PAGE, tree).expect("save page");
    assert!(notes.save_page("", Vec::new()).is_err(), "no empty names");
    // A change on disk, re-indexed by the watcher.
    std::fs::write(
        graph.join("pages").join(format!("{PAGE} two.md")),
        format!("- {}\n", canary_text()),
    )
    .expect("write");
    assert!(notes.page(&format!("{PAGE} three")).is_ok());

    let vault = dir.path().join(WORKSPACE);
    let scene = json!({
        "type": "excalidraw", "version": 2, "source": "next-agent",
        "elements": [{ "type": "text", "text": canary_text() }],
        "appState": {}, "files": {}
    })
    .to_string();
    let board = boards::create(&vault, BOARD, Some(&scene)).expect("create board");
    let saved = boards::save(&vault, BOARD, &scene, Some(&board.revision)).expect("save board");
    assert!(boards::save(&vault, BOARD, &scene, Some("stale")).is_err());
    assert!(boards::save(&vault, BOARD, "not json", Some(&saved)).is_err());
    assert!(boards::open(&vault, &format!("{BOARD} missing")).is_err());

    // Give the watcher time to see the file.
    std::thread::sleep(Duration::from_millis(1500));
    assert_clean(log);
    let all = events_where(log, |_| true);
    assert_has(
        &all,
        &[
            "graph_indexed",
            "graph_reindexed",
            "board_saved",
            "board_conflict",
        ],
    );
}

/// The Design Studio's preview proxy refusing a foreign host, and a dead
/// dev server: a status and a constant reason, never the host, the path or a
/// header.
#[test]
fn canary_design_preview_refusals_stay_out_of_the_log() {
    let log = log();
    // Nothing listens on port 9.
    let target = next_agent_design::ssrf::check_preview_url("http://localhost:9/").expect("url");
    let proxy = PreviewProxy::start(target, String::new()).expect("proxy");
    let origin = proxy.origin();
    let addr = origin.trim_start_matches("http://");
    let ask = |host: &str| {
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(
            stream,
            "GET /{FILE}?q={PROMPT_MARK} HTTP/1.1\r\nHost: {host}\r\nCookie: s={SK_ANT}\r\n\r\n"
        )
        .expect("write");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        out
    };
    assert!(ask(&format!("{WORKSPACE}.example")).starts_with("HTTP/1.1 421"));
    assert!(ask(addr).starts_with("HTTP/1.1 502"));
    drop(proxy);

    assert_clean(log);
    let blocked: Vec<Value> = entries(log)
        .into_iter()
        .filter(|e| e["event"] == "preview_blocked" && e["fields"]["reason"] == "wrong_host")
        .collect();
    assert!(!blocked.is_empty(), "no preview_blocked");
    assert_eq!(blocked[0]["fields"]["status"], 421);
    assert_has(&events_where(log, |_| true), &["preview_upstream_down"]);
}

/// The memory editor (the user's own edit) and a cron job whose name and
/// prompt are canaries, dispatched as its own task.
#[test]
fn canary_memory_editor_and_cron_run_stay_out_of_the_log() {
    let log = log();
    let dir = tempfile::tempdir().expect("tmp");
    let (svc, ws) = service(dir.path());
    svc.memory_save(&ws, "user", &format!("{PROMPT_MARK} I am {EMAIL}"))
        .expect("memory");
    assert!(svc
        .memory_save(&ws, "memory", &canary_text().repeat(40))
        .is_err());

    let job = svc
        .cron_save(&CronInput {
            workspace_id: ws,
            name: Some(format!("{PAGE} nightly")),
            prompt: canary_text(),
            schedule: "every 1h".into(),
            mode: Some("plan".into()),
            ..CronInput::default()
        })
        .expect("cron");
    let run = cron::run_now(&svc.store().expect("store"), &job.id, sessions::now()).expect("run");
    let task = svc
        .cron_dispatch(&run, allowing_sink(&svc))
        .expect("dispatch");
    svc.task_wait(&task);

    assert_clean(log);
    assert_has(&events_where(log, |_| true), &["memory_saved"]);
    assert_has(
        &events_where(log, |e| of(e, "cron_id", &job.id)),
        &["cron_fired"],
    );
    assert_has(
        &events_where(log, |e| of(e, "task_id", &task)),
        &["task_started", "task_finished"],
    );
}
