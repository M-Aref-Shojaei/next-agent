//! Router client against a mock OpenAI-compatible server (wiremock):
//! health ok / slow / timeout / 401 / down, SSE streaming, retries, and the
//! `BuiltinExecutor` tool loop end-to-end.

use next_agent_core::agent::TaskSpec;
use next_agent_core::broker::context_for;
use next_agent_core::control::{spawn_task, TaskLaunch};
use next_agent_core::llm::{ChatMessage, ChatRequest, CompletionClient, LlmError};
use next_agent_core::router::{RetryPolicy, RouterClient, RouterSettings};
use next_agent_core::task::{TaskId, TaskStatus};
use next_agent_core::{BuiltinExecutor, LocalTools, Store, Workspace};
use next_agent_permission_engine::Mode;
use next_agent_process_manager::{HealthMonitor, HealthReport, HealthStatus, MonitorConfig};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn settings(server: &MockServer) -> RouterSettings {
    let mut settings = RouterSettings::default()
        .with_base_url(&format!("{}/v1", server.uri()))
        .expect("url");
    settings.health_timeout = Duration::from_millis(300);
    settings
}

fn client(server: &MockServer, key: Option<&str>) -> RouterClient {
    RouterClient::new(settings(server), key.map(str::to_string))
        .expect("client")
        .with_retry(RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(10),
        })
}

fn models() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "object": "list",
        "data": [{"id": "gpt-test", "object": "model"}, {"id": "claude-test", "object": "model"}]
    }))
}

fn sse(chunks: &[serde_json::Value]) -> ResponseTemplate {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str("data: ");
        body.push_str(&chunk.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

fn delta(content: &str) -> serde_json::Value {
    json!({"choices": [{"index": 0, "delta": {"content": content}}]})
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "gpt-test".into(),
        messages: vec![ChatMessage::user("hi")],
        tools: vec![],
    }
}

#[tokio::test]
async fn health_ok_lists_models_with_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(models())
        .expect(1..)
        .mount(&server)
        .await;
    let client = client(&server, Some("sk-test"));
    assert_eq!(
        client.list_models().await.expect("models"),
        ["gpt-test", "claude-test"]
    );
    let report = client.health().await;
    assert_eq!(report.status, HealthStatus::Ok, "{report:?}");
    assert_eq!(report.detail, "2 models");
    assert!(report.latency_ms.is_some());
}

#[tokio::test]
async fn health_401_is_degraded() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let report = client(&server, None).health().await;
    assert_eq!(report.status, HealthStatus::Degraded);
    assert!(report.detail.contains("unauthorized"));
}

#[tokio::test]
async fn health_timeout_is_down() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(models().set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let report = client(&server, None).health().await;
    assert_eq!(report.status, HealthStatus::Down, "{report:?}");
    assert_eq!(report.detail, "timed out");
}

#[tokio::test]
async fn health_slow_answer_is_degraded() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(models().set_delay(Duration::from_millis(150)))
        .mount(&server)
        .await;
    let mut settings = settings(&server);
    settings.slow_threshold = Duration::from_millis(50);
    let report = RouterClient::new(settings, None)
        .expect("client")
        .health()
        .await;
    assert_eq!(report.status, HealthStatus::Degraded);
    assert!(report.detail.starts_with("slow"));
}

/// Settings pointing at a port nothing listens on.
fn closed_port_settings() -> RouterSettings {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    RouterSettings::default()
        .with_base_url(&format!("http://127.0.0.1:{port}/v1"))
        .expect("url")
}

#[tokio::test]
async fn health_connection_refused_is_down() {
    let report = RouterClient::new(closed_port_settings(), None)
        .expect("client")
        .health()
        .await;
    assert_eq!(report.status, HealthStatus::Down, "{report:?}");
    assert_eq!(report.detail, "connection failed");
}

#[tokio::test]
async fn health_empty_model_list_is_degraded() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, None).health().await.status,
        HealthStatus::Degraded
    );
}

async fn wait_for(rx: &tokio::sync::watch::Receiver<HealthReport>, want: HealthStatus) {
    let mut rx = rx.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let report = rx.borrow_and_update().clone();
            // `checks == 0` is the placeholder before the first probe.
            if report.checks > 0 && report.status == want {
                return;
            }
            rx.changed().await.expect("monitor alive");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("status never became {want:?} within 5 s"));
}

fn fast_monitor(client: RouterClient) -> HealthMonitor {
    HealthMonitor::spawn(
        Arc::new(client),
        MonitorConfig {
            interval: Duration::from_millis(100),
            timeout: Duration::from_secs(3),
        },
    )
}

#[tokio::test]
async fn monitor_reflects_router_status_changes_within_five_seconds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .respond_with(models())
        .mount(&server)
        .await;
    let monitor = fast_monitor(client(&server, None));
    wait_for(&monitor.subscribe(), HealthStatus::Ok).await;
    // Router starts rejecting requests (e.g. key revoked) → degraded.
    server.reset().await;
    Mock::given(path("/v1/models"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    wait_for(&monitor.subscribe(), HealthStatus::Degraded).await;
    // Recovery is picked up too.
    server.reset().await;
    Mock::given(path("/v1/models"))
        .respond_with(models())
        .mount(&server)
        .await;
    wait_for(&monitor.subscribe(), HealthStatus::Ok).await;
}

#[tokio::test]
async fn monitor_reports_unreachable_router_as_down() {
    let client = RouterClient::new(closed_port_settings(), None).expect("client");
    let monitor = fast_monitor(client);
    wait_for(&monitor.subscribe(), HealthStatus::Down).await;
    assert_eq!(
        monitor.current().status.service_state(),
        next_agent_process_manager::ServiceState::Crashed
    );
}

#[tokio::test]
async fn chat_streams_text_deltas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("\"stream\":true"))
        .respond_with(sse(&[
            json!({"choices": [{"index": 0, "delta": {"role": "assistant"}}]}),
            delta("Hel"),
            delta("lo"),
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
        ]))
        .expect(1)
        .mount(&server)
        .await;
    let mut deltas = Vec::new();
    let completion = client(&server, None)
        .chat(&request(), &mut |text| deltas.push(text.to_string()))
        .await
        .expect("chat");
    assert_eq!(deltas, ["Hel", "lo"]);
    assert_eq!(completion.content, "Hello");
    assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn chat_streams_tool_calls() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(sse(&[
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":""}}]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.rs\"}"}}]}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ]))
        .mount(&server)
        .await;
    let completion = client(&server, None)
        .chat(&request(), &mut |_| {})
        .await
        .expect("chat");
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].id, "call_1");
    assert_eq!(
        completion.tool_calls[0].function.arguments,
        "{\"path\":\"a.rs\"}"
    );
}

#[tokio::test]
async fn chat_401_is_unauthorized_without_retry() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .expect(1)
        .mount(&server)
        .await;
    let error = client(&server, Some("wrong"))
        .chat(&request(), &mut |_| {})
        .await
        .expect_err("401");
    assert_eq!(error, LlmError::Unauthorized);
}

#[tokio::test]
async fn chat_unknown_model_is_reported() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let error = client(&server, None)
        .chat(&request(), &mut |_| {})
        .await
        .expect_err("404");
    assert_eq!(error, LlmError::ModelNotFound("gpt-test".into()));
}

#[tokio::test]
async fn chat_retries_5xx_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(sse(&[delta("ok")]))
        .with_priority(2)
        .mount(&server)
        .await;
    let completion = client(&server, None)
        .chat(&request(), &mut |_| {})
        .await
        .expect("third attempt succeeds");
    assert_eq!(completion.content, "ok");
}

#[tokio::test]
async fn chat_gives_up_after_retry_budget() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .expect(3)
        .mount(&server)
        .await;
    let error = client(&server, None)
        .chat(&request(), &mut |_| {})
        .await
        .expect_err("429");
    assert!(matches!(error, LlmError::Http { status: 429, .. }));
}

#[tokio::test]
async fn non_streaming_json_answers_are_accepted() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "plain"}, "finish_reason": "stop"}]
        })))
        .mount(&server)
        .await;
    let mut seen = String::new();
    let completion = client(&server, None)
        .chat(&request(), &mut |text| seen.push_str(text))
        .await
        .expect("chat");
    assert_eq!(
        (completion.content.as_str(), seen.as_str()),
        ("plain", "plain")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_bridge_works_from_a_plain_thread() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(sse(&[delta("bridged")]))
        .mount(&server)
        .await;
    let client = client(&server, None);
    let completion = std::thread::spawn(move || client.complete(&request(), &mut |_| {}))
        .join()
        .expect("thread")
        .expect("chat");
    assert_eq!(completion.content, "bridged");
}

/// Workspace with `src/lib.rs`, a metadata db, and a registered task row.
fn builtin_fixture() -> (Workspace, std::path::PathBuf, TaskId) {
    let dir = std::env::temp_dir().join(format!("na-builtin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project/src")).expect("dirs");
    std::fs::write(dir.join("project/src/lib.rs"), "pub fn answer() {}\n").expect("seed");
    let workspace = Workspace::open(dir.join("project")).expect("workspace");
    let db = dir.join("meta.db");
    let id = TaskId::from_seed("builtin-e2e");
    {
        let store = Store::open(&db).expect("store");
        store.upsert_workspace(&workspace).expect("ws");
        store
            .create_task(
                id.as_str(),
                workspace.id(),
                "e2e",
                "what does it export?",
                "auto",
            )
            .expect("task");
    }
    (workspace, db, id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_executor_runs_a_tool_loop_against_the_router() {
    let server = MockServer::start().await;
    // Second turn: the conversation now contains the tool result.
    Mock::given(path("/v1/chat/completions"))
        .and(body_string_contains("\"role\":\"tool\""))
        .respond_with(sse(&[delta("The crate exposes "), delta("`answer`.")]))
        .with_priority(1)
        .mount(&server)
        .await;
    // First turn: ask to read a file (relative path).
    Mock::given(path("/v1/chat/completions"))
        .respond_with(sse(&[
            delta("Let me look."),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_r","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/lib.rs\"}"}}]},"finish_reason":"tool_calls"}]}),
        ]))
        .with_priority(2)
        .mount(&server)
        .await;

    let (workspace, db, id) = builtin_fixture();

    let router = Arc::new(client(&server, None));
    let handle = spawn_task(TaskLaunch {
        db_path: db.clone(),
        spec: TaskSpec {
            id: id.clone(),
            prompt: "what does it export?".into(),
            mode: "auto",
            model: "gpt-test".into(),
        },
        mode: Mode::Auto,
        context: context_for(
            workspace.root().to_path_buf(),
            workspace.id().to_string(),
            None,
        ),
        executor: Box::new(BuiltinExecutor::new(router, workspace.root())),
        tools: Box::new(LocalTools::new()),
    });
    let (events, status) = tokio::task::spawn_blocking(move || {
        let events: Vec<_> = handle.events().iter().collect();
        (events, handle.join())
    })
    .await
    .expect("join");
    assert_eq!(status, Ok(TaskStatus::Completed));

    let kinds: Vec<&str> = events.iter().map(|e| e.kind.kind()).collect();
    assert_eq!(
        kinds,
        [
            "task_started",
            "message_delta",
            "message",
            "tool_requested",
            "permission_decided",
            "tool_started",
            "tool_finished",
            "message_delta",
            "message_delta",
            "message",
            "task_completed",
        ]
    );
    assert_eq!(events[0].data()["executor"], "builtin");
    assert_eq!(events[3].data()["tool"], "read_file");
    assert_eq!(
        events[3].data()["args"]["path"],
        workspace.root().join("src/lib.rs").display().to_string()
    );
    assert_eq!(events[6].data()["summary"], "pub fn answer() {}\n");
    assert_eq!(
        events.last().expect("last").data()["summary"],
        "The crate exposes `answer`."
    );
    let stored = Store::open(&db)
        .expect("store")
        .task_events(id.as_str())
        .expect("events");
    assert_eq!(stored.len(), events.len());

    // Both turns hit the router; the second carried the tool result back.
    let requests = server.received_requests().await.expect("recording");
    assert_eq!(requests.len(), 2);
    let second: serde_json::Value = requests[1].body_json().expect("json");
    let tool_message = second["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|m| m["role"] == "tool"))
        .expect("tool message");
    assert_eq!(tool_message["tool_call_id"], "call_r");
    assert_eq!(second["tools"].as_array().map(Vec::len), Some(7));
}
