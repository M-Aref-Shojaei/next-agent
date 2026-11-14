//! Gateway tests against wiremock providers: per provider ok / 401 / 429 →
//! fallback / streaming / tool calls, plus combo, cooldown, role, saver and
//! usage behaviour.

use super::*;
use crate::llm::ChatMessage;
use crate::router::combo::CooldownPolicy;
use crate::router::providers::MemoryKeys;
use crate::store::ComboRow;
use serde_json::json;
use std::fmt::Write as _;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct MemoryUsage(Mutex<Vec<UsageRow>>);

impl UsageSink for MemoryUsage {
    fn record(&self, row: &UsageRow) {
        self.0.lock().expect("lock").push(row.clone());
    }
}

impl MemoryUsage {
    fn rows(&self) -> Vec<UsageRow> {
        self.0.lock().expect("lock").clone()
    }
}

fn conn(id: &str, provider: &str, kind: ProviderKind, base: &str, priority: i64) -> Connection {
    Connection {
        id: id.into(),
        provider: provider.into(),
        auth_type: "api_key".into(),
        name: format!("{provider}-{id}"),
        priority,
        is_active: true,
        kind,
        base_url: base.into(),
        models: Vec::new(),
        needs_key: true,
    }
}

fn combo(name: &str, models: &[&str], strategy: Option<Strategy>) -> Combo {
    Combo {
        id: format!("id-{name}"),
        name: name.into(),
        models: models.iter().map(|m| (*m).to_string()).collect(),
        strategy,
        sticky_limit: 1,
    }
}

struct Harness {
    gateway: Gateway,
    usage: Arc<MemoryUsage>,
}

fn harness(connections: Vec<Connection>, combos: Vec<Combo>, settings: GatewaySettings) -> Harness {
    let keys: Vec<(String, String)> = connections
        .iter()
        .map(|c| (c.id.clone(), format!("key-{}", c.id)))
        .collect();
    let refs: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let config = GatewayConfig {
        connections,
        combos,
        settings,
        pricing: Pricing::builtin(),
        legacy_model: None,
    };
    let usage = Arc::new(MemoryUsage::default());
    let gateway = Gateway::new(config, Arc::new(MemoryKeys::with(&refs)))
        .expect("gateway")
        .with_state(Arc::new(GatewayState::new(CooldownPolicy {
            base: Duration::from_millis(20),
            max: Duration::from_millis(200),
        })))
        .with_usage(usage.clone());
    Harness { gateway, usage }
}

fn sse(events: &[Value]) -> ResponseTemplate {
    let mut body = String::new();
    for event in events {
        if let Some(name) = event.get("type").and_then(Value::as_str) {
            let _ = writeln!(body, "event: {name}");
        }
        let _ = write!(body, "data: {event}\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

fn openai_text(text: &str) -> ResponseTemplate {
    sse(&[
        json!({"choices":[{"delta":{"role":"assistant","content":text}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":100,"prompt_tokens_details":{"cached_tokens":200}}}),
    ])
}

fn request(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
}

async fn run(gateway: &Gateway, body: Value) -> (Result<Completion, LlmError>, String) {
    let mut streamed = String::new();
    let result = gateway
        .chat_value(body, &RequestOptions::default(), &mut |d| {
            streamed.push_str(d);
        })
        .await;
    (result, streamed)
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
        .collect()
}

// ---- OpenAI-compatible ----------------------------------------------------------

#[tokio::test]
async fn openai_streams_text_and_records_usage_with_cost() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer key-o1"))
        .respond_with(openai_text("Hello"))
        .expect(1)
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "o1",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let (result, streamed) = run(&h.gateway, request("openai/gpt-4o")).await;
    let completion = result.expect("completion");
    assert_eq!(completion.content, "Hello");
    assert_eq!(streamed, "Hello");
    let sent = &bodies(&server).await[0];
    assert_eq!(sent["model"], "gpt-4o");
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["stream_options"]["include_usage"], true);
    let rows = h.usage.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, 200);
    assert_eq!(rows[0].prompt_tokens, 1000);
    assert_eq!(rows[0].cached_tokens, 200);
    assert_eq!(rows[0].role, "chat");
    assert!(rows[0].cost_known);
    // gpt-4o: (800·2.5 + 200·1.25 + 100·10) / 1e6
    assert!((rows[0].cost - 0.003_25).abs() < 1e-9, "{}", rows[0].cost);
}

#[tokio::test]
async fn openai_tool_calls_stream_in_fragments() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(sse(&[
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ]))
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "o1",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let completion = run(&h.gateway, request("openai/gpt-4o"))
        .await
        .0
        .expect("completion");
    assert_eq!(completion.tool_calls[0].function.name, "read_file");
    assert_eq!(
        completion.tool_calls[0].function.arguments,
        "{\"path\":\"a\"}"
    );
    assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
}

#[tokio::test]
async fn openai_json_answers_are_accepted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices":[{"message":{"content":"plain","tool_calls":[{"id":"c","type":"function","function":{"name":"list_dir","arguments":"{}"}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":5,"completion_tokens":2}
        })))
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "o1",
            "ollama",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let (result, streamed) = run(&h.gateway, request("ollama/llama3.2")).await;
    let completion = result.expect("completion");
    assert_eq!(streamed, "plain");
    assert_eq!(completion.tool_calls[0].function.name, "list_dir");
    let row = &h.usage.rows()[0];
    assert!(
        row.cost_known && row.cost == 0.0,
        "local providers are free"
    );
}

#[tokio::test]
async fn openai_401_is_unauthorized_and_cools_the_account() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .expect(1)
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "o1",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    assert_eq!(
        run(&h.gateway, request("openai/gpt-4o")).await.0,
        Err(LlmError::Unauthorized)
    );
    assert!(!h.gateway.state().available("o1", "gpt-4o"));
    // While cooling down the account is skipped without a request.
    assert!(matches!(
        run(&h.gateway, request("openai/gpt-4o")).await.0,
        Err(LlmError::Unavailable(_))
    ));
    assert_eq!(h.usage.rows()[0].status, 401);
}

#[tokio::test]
async fn openai_429_falls_back_to_the_next_model() {
    let bad = MockServer::start().await;
    let good = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .expect(1)
        .mount(&bad)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("from b"))
        .mount(&good)
        .await;
    let h = harness(
        vec![
            conn(
                "a",
                "groq",
                ProviderKind::OpenAi,
                &format!("{}/v1", bad.uri()),
                1,
            ),
            conn(
                "b",
                "deepseek",
                ProviderKind::OpenAi,
                &format!("{}/v1", good.uri()),
                1,
            ),
        ],
        vec![combo(
            "duo",
            &["groq/llama", "deepseek/deepseek-chat"],
            None,
        )],
        GatewaySettings::default(),
    );
    let completion = run(&h.gateway, request("duo")).await.0.expect("fallback");
    assert_eq!(completion.content, "from b");
    assert!(
        !h.gateway.state().available("a", "llama"),
        "model locked after 429"
    );
    assert!(h.gateway.state().available("a", "other"));
    let rows = h.usage.rows();
    assert_eq!((rows[0].status, rows[1].status), (429, 200));
    assert_eq!(rows[1].combo.as_deref(), Some("duo"));
}

// ---- Anthropic --------------------------------------------------------------------

fn anthropic_text(text: &str) -> ResponseTemplate {
    sse(&[
        json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":90}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}),
        json!({"type":"message_stop"}),
    ])
}

#[tokio::test]
async fn anthropic_streams_with_headers_translation_and_cache_breakpoints() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "key-an"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(anthropic_text("Bonjour"))
        .expect(1)
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "an",
            "anthropic",
            ProviderKind::Anthropic,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let request = ChatRequest {
        model: "anthropic/claude-sonnet-4-5".into(),
        messages: vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user("hi"),
        ],
        tools: crate::builtin::tool_schemas(),
    };
    let mut streamed = String::new();
    let completion = h
        .gateway
        .chat(&request, &RequestOptions::default(), &mut |d| {
            streamed.push_str(d);
        })
        .await
        .expect("completion");
    assert_eq!(completion.content, "Bonjour");
    assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
    let sent = &bodies(&server).await[0];
    assert_eq!(sent["model"], "claude-sonnet-4-5");
    assert_eq!(sent["max_tokens"], 16384);
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["system"][0]["text"], "You are helpful.");
    assert_eq!(sent["system"][0]["cache_control"]["type"], "ephemeral");
    let tools = sent["tools"].as_array().expect("tools");
    assert!(tools
        .last()
        .is_some_and(|t| t.get("cache_control").is_some()));
    let row = &h.usage.rows()[0];
    assert_eq!(
        (row.prompt_tokens, row.cached_tokens, row.completion_tokens),
        (100, 90, 7)
    );
    assert!(row.cost_known);
}

#[tokio::test]
async fn anthropic_tool_calls_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(sse(&[
            json!({"type":"message_start","message":{"usage":{"input_tokens":3}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"list_dir","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\".\"}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ]))
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "an",
            "anthropic",
            ProviderKind::Anthropic,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let body = json!({"model": "anthropic/claude-haiku-4-5", "messages": [
        {"role": "user", "content": "list"},
        {"role": "assistant", "content": null, "tool_calls": [{"id": "toolu_0", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a\"}"}}]},
        {"role": "tool", "tool_call_id": "toolu_0", "content": "file body"}
    ]});
    let completion = run(&h.gateway, body).await.0.expect("completion");
    assert_eq!(completion.tool_calls[0].id, "toolu_1");
    assert_eq!(
        completion.tool_calls[0].function.arguments,
        "{\"path\":\".\"}"
    );
    assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
    let sent = &bodies(&server).await[0];
    assert_eq!(sent["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(sent["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(sent["messages"][2]["content"][0]["tool_use_id"], "toolu_0");
}

#[tokio::test]
async fn anthropic_401_and_429_fall_back_to_gemini() {
    let anthropic = MockServer::start().await;
    let gemini = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("x-api-key", "key-an1"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_string(r#"{"error":{"type":"authentication_error"}}"#),
        )
        .expect(1)
        .mount(&anthropic)
        .await;
    Mock::given(method("POST"))
        .and(header("x-api-key", "key-an2"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .expect(1)
        .mount(&anthropic)
        .await;
    Mock::given(method("POST"))
        .respond_with(gemini_text("hola"))
        .expect(1)
        .mount(&gemini)
        .await;
    let h = harness(
        vec![
            conn(
                "an1",
                "anthropic",
                ProviderKind::Anthropic,
                &format!("{}/v1", anthropic.uri()),
                1,
            ),
            conn(
                "an2",
                "anthropic",
                ProviderKind::Anthropic,
                &format!("{}/v1", anthropic.uri()),
                2,
            ),
            conn(
                "ge",
                "gemini",
                ProviderKind::Gemini,
                &format!("{}/v1beta", gemini.uri()),
                1,
            ),
        ],
        vec![combo(
            "smart-ish",
            &["anthropic/claude-sonnet-4-5", "gemini/gemini-2.5-flash"],
            None,
        )],
        GatewaySettings::default(),
    );
    let completion = run(&h.gateway, request("smart-ish"))
        .await
        .0
        .expect("fallback");
    assert_eq!(completion.content, "hola!");
    let statuses: Vec<i64> = h.usage.rows().iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [401, 429, 200],
        "both accounts tried, then the next model"
    );
}

// ---- Gemini -----------------------------------------------------------------------

fn gemini_text(text: &str) -> ResponseTemplate {
    sse(&[
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"!"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3}}),
    ])
}

#[tokio::test]
async fn gemini_streams_text_with_key_header_and_sse_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .and(query_param("alt", "sse"))
        .and(header("x-goog-api-key", "key-ge"))
        .respond_with(gemini_text("Hi"))
        .expect(1)
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "ge",
            "gemini",
            ProviderKind::Gemini,
            &format!("{}/v1beta", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let (result, streamed) = run(&h.gateway, request("gemini/gemini-2.5-flash")).await;
    assert_eq!(result.expect("completion").content, "Hi!");
    assert_eq!(streamed, "Hi!");
    let sent = &bodies(&server).await[0];
    assert_eq!(sent["contents"][0]["role"], "user");
    let row = &h.usage.rows()[0];
    assert_eq!((row.prompt_tokens, row.completion_tokens), (12, 3));
}

#[tokio::test]
async fn gemini_tool_calls_map_names_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(sse(&[json!({"candidates":[{"content":{"parts":[
            {"functionCall":{"name":"mcp_fs_read","args":{"path":"x"}}}
        ]},"finishReason":"STOP"}]})]))
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "ge",
            "gemini",
            ProviderKind::Gemini,
            &format!("{}/v1beta", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let body = json!({"model": "gemini/gemini-2.5-pro", "messages": [{"role": "user", "content": "read"}],
        "tools": [{"type": "function", "function": {"name": "mcp:fs/read", "parameters": {"type": "object"}}}]});
    let completion = run(&h.gateway, body).await.0.expect("completion");
    assert_eq!(completion.tool_calls[0].function.name, "mcp:fs/read");
    assert_eq!(
        completion.tool_calls[0].function.arguments,
        "{\"path\":\"x\"}"
    );
    assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
}

#[tokio::test]
async fn gemini_401_then_429_fallback() {
    let gemini = MockServer::start().await;
    let openai = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403).set_body_string("API key not valid"))
        .mount(&gemini)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("ok"))
        .mount(&openai)
        .await;
    let h = harness(
        vec![
            conn(
                "ge",
                "gemini",
                ProviderKind::Gemini,
                &format!("{}/v1beta", gemini.uri()),
                1,
            ),
            conn(
                "o",
                "openai",
                ProviderKind::OpenAi,
                &format!("{}/v1", openai.uri()),
                1,
            ),
        ],
        vec![combo(
            "c",
            &["gemini/gemini-2.5-flash", "openai/gpt-4o-mini"],
            None,
        )],
        GatewaySettings::default(),
    );
    assert_eq!(
        run(&h.gateway, request("c")).await.0.expect("ok").content,
        "ok"
    );
    assert_eq!(
        run(&h.gateway, request("gemini/gemini-2.5-flash"))
            .await
            .0
            .err()
            .map(|e| matches!(e, LlmError::Unavailable(_))),
        Some(true)
    );

    let limited = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("quota"))
        .mount(&limited)
        .await;
    let h = harness(
        vec![
            conn(
                "ge",
                "gemini",
                ProviderKind::Gemini,
                &format!("{}/v1beta", limited.uri()),
                1,
            ),
            conn(
                "o",
                "openai",
                ProviderKind::OpenAi,
                &format!("{}/v1", openai.uri()),
                1,
            ),
        ],
        vec![combo(
            "c",
            &["gemini/gemini-2.5-flash", "openai/gpt-4o-mini"],
            None,
        )],
        GatewaySettings::default(),
    );
    assert_eq!(
        run(&h.gateway, request("c")).await.0.expect("ok").content,
        "ok"
    );
}

// ---- combos, fall-through and cooldown -----------------------------------------------

#[tokio::test]
async fn empty_streams_fall_through_but_the_last_model_answers() {
    let empty = MockServer::start().await;
    let good = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(sse(&[
            json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        ]))
        .mount(&empty)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("filled"))
        .mount(&good)
        .await;
    let h = harness(
        vec![
            conn(
                "e",
                "groq",
                ProviderKind::OpenAi,
                &format!("{}/v1", empty.uri()),
                1,
            ),
            conn(
                "g",
                "xai",
                ProviderKind::OpenAi,
                &format!("{}/v1", good.uri()),
                1,
            ),
        ],
        vec![
            combo("fill", &["groq/m", "xai/grok-4"], None),
            combo("only-empty", &["groq/m"], None),
        ],
        GatewaySettings::default(),
    );
    assert_eq!(
        run(&h.gateway, request("fill"))
            .await
            .0
            .expect("ok")
            .content,
        "filled"
    );
    let last = run(&h.gateway, request("only-empty"))
        .await
        .0
        .expect("empty but ok");
    assert_eq!(last.content, "");
}

#[tokio::test]
async fn request_scoped_errors_return_the_first_one() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request from a"))
        .expect(1)
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("b broke"))
        .expect(1)
        .mount(&b)
        .await;
    let h = harness(
        vec![
            conn(
                "a",
                "groq",
                ProviderKind::OpenAi,
                &format!("{}/v1", a.uri()),
                1,
            ),
            conn(
                "b",
                "xai",
                ProviderKind::OpenAi,
                &format!("{}/v1", b.uri()),
                1,
            ),
        ],
        vec![combo("pair", &["groq/m", "xai/n"], None)],
        GatewaySettings::default(),
    );
    match run(&h.gateway, request("pair")).await.0 {
        Err(LlmError::Http { status: 400, body }) => assert!(body.contains("from a")),
        other => panic!("expected the first 400, got {other:?}"),
    }
}

#[tokio::test]
async fn everything_down_is_a_503() {
    let h = harness(
        vec![
            conn(
                "a",
                "groq",
                ProviderKind::OpenAi,
                "http://127.0.0.1:9/v1",
                1,
            ),
            conn("b", "xai", ProviderKind::OpenAi, "http://127.0.0.1:9/v1", 1),
        ],
        vec![combo("dead", &["groq/m", "xai/n"], None)],
        GatewaySettings::default(),
    );
    match run(&h.gateway, request("dead")).await.0 {
        Err(LlmError::Http { status: 503, body }) => {
            assert!(body.contains("all 2 models failed"), "{body}");
        }
        other => panic!("expected 503, got {other:?}"),
    }
    assert!(h.usage.rows().iter().all(|r| r.status == 0));
}

#[tokio::test]
async fn gateway_5xx_short_cooldown_then_next_model() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("b"))
        .mount(&b)
        .await;
    let h = harness(
        vec![
            conn(
                "a",
                "groq",
                ProviderKind::OpenAi,
                &format!("{}/v1", a.uri()),
                1,
            ),
            conn(
                "b",
                "xai",
                ProviderKind::OpenAi,
                &format!("{}/v1", b.uri()),
                1,
            ),
        ],
        vec![combo("pair", &["groq/m", "xai/n"], None)],
        GatewaySettings::default(),
    );
    let started = Instant::now();
    assert_eq!(
        run(&h.gateway, request("pair"))
            .await
            .0
            .expect("ok")
            .content,
        "b"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(20),
        "waited out the short cooldown"
    );
    let statuses: Vec<i64> = h.usage.rows().iter().map(|r| r.status).collect();
    assert_eq!(statuses, [503, 200]);
}

#[tokio::test]
async fn round_robin_rotates_and_global_strategy_applies() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("A"))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("B"))
        .mount(&b)
        .await;
    let connections = vec![
        conn(
            "a",
            "groq",
            ProviderKind::OpenAi,
            &format!("{}/v1", a.uri()),
            1,
        ),
        conn(
            "b",
            "xai",
            ProviderKind::OpenAi,
            &format!("{}/v1", b.uri()),
            1,
        ),
    ];
    let h = harness(
        connections.clone(),
        vec![combo(
            "rr",
            &["groq/m", "xai/n"],
            Some(Strategy::RoundRobin),
        )],
        GatewaySettings::default(),
    );
    let mut answers = Vec::new();
    for _ in 0..4 {
        answers.push(run(&h.gateway, request("rr")).await.0.expect("ok").content);
    }
    assert_eq!(answers, ["A", "B", "A", "B"]);
    let global = GatewaySettings {
        combo_strategy: Strategy::RoundRobin,
        ..GatewaySettings::default()
    };
    let h = harness(
        connections,
        vec![combo("g", &["groq/m", "xai/n"], None)],
        global,
    );
    run(&h.gateway, request("g")).await.0.expect("first");
    assert_eq!(
        run(&h.gateway, request("g"))
            .await
            .0
            .expect("second")
            .content,
        "B"
    );
}

#[tokio::test]
async fn second_account_takes_over_after_401_and_missing_keys_are_skipped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer key-p1"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer key-p3"))
        .respond_with(openai_text("third account"))
        .mount(&server)
        .await;
    let base = format!("{}/v1", server.uri());
    let h = harness(
        vec![
            conn("p1", "openai", ProviderKind::OpenAi, &base, 1),
            conn("p2", "openai", ProviderKind::OpenAi, &base, 2),
            conn("p3", "openai", ProviderKind::OpenAi, &base, 3),
        ],
        vec![],
        GatewaySettings::default(),
    );
    h.gateway.keys.delete("p2").expect("drop key");
    let completion = run(&h.gateway, request("openai/gpt-4o"))
        .await
        .0
        .expect("ok");
    assert_eq!(completion.content, "third account");
    assert_eq!(
        server.received_requests().await.map(|r| r.len()),
        Some(2),
        "p2 has no key: no request"
    );
}

#[tokio::test]
async fn broken_stream_after_content_does_not_fall_back() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {not json\n\n"),
        )
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("b"))
        .expect(0)
        .mount(&b)
        .await;
    let h = harness(
        vec![
            conn(
                "a",
                "groq",
                ProviderKind::OpenAi,
                &format!("{}/v1", a.uri()),
                1,
            ),
            conn(
                "b",
                "xai",
                ProviderKind::OpenAi,
                &format!("{}/v1", b.uri()),
                1,
            ),
        ],
        vec![combo("pair", &["groq/m", "xai/n"], None)],
        GatewaySettings::default(),
    );
    let (result, streamed) = run(&h.gateway, request("pair")).await;
    assert!(matches!(result, Err(LlmError::Protocol(_))));
    assert_eq!(streamed, "partial");
}

#[tokio::test]
async fn vision_requests_try_capable_models_first() {
    let text_only = MockServer::start().await;
    let vision = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("text"))
        .expect(0)
        .mount(&text_only)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("saw it"))
        .expect(1)
        .mount(&vision)
        .await;
    let h = harness(
        vec![
            conn(
                "d",
                "deepseek",
                ProviderKind::OpenAi,
                &format!("{}/v1", text_only.uri()),
                1,
            ),
            conn(
                "o",
                "openai",
                ProviderKind::OpenAi,
                &format!("{}/v1", vision.uri()),
                1,
            ),
        ],
        vec![combo(
            "mixed",
            &["deepseek/deepseek-chat", "openai/gpt-4o"],
            None,
        )],
        GatewaySettings::default(),
    );
    let body = json!({"model": "mixed", "messages": [{"role": "user", "content": [
        {"type": "text", "text": "what is this"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}}
    ]}]});
    assert_eq!(run(&h.gateway, body).await.0.expect("ok").content, "saw it");
}

// ---- roles, savers, legacy settings -----------------------------------------------------

#[tokio::test]
async fn roles_resolve_empty_models_and_savers_obey_per_request_off() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("ok"))
        .mount(&server)
        .await;
    let mut roles = BTreeMap::new();
    roles.insert("review".to_string(), "combo:reviewers".to_string());
    roles.insert("chat".to_string(), "openai/gpt-4o-mini".to_string());
    let settings = GatewaySettings {
        caveman: Some(Level::Full),
        roles,
        ..GatewaySettings::default()
    };
    let h = harness(
        vec![conn(
            "o",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![combo("reviewers", &["openai/gpt-4.1"], None)],
        settings,
    );
    let reviewer = h.gateway.clone().with_role("review");
    run(&reviewer, request("")).await.0.expect("review");
    run(&h.gateway, request("")).await.0.expect("chat");
    let mut quiet = String::new();
    h.gateway
        .chat_value(
            request(""),
            &RequestOptions {
                role: Some("summary".into()),
                token_saver: false,
            },
            &mut |d| quiet.push_str(d),
        )
        .await
        .expect("summary falls back to chat");
    let sent = bodies(&server).await;
    assert_eq!(sent[0]["model"], "gpt-4.1");
    assert_eq!(sent[1]["model"], "gpt-4o-mini");
    assert_eq!(sent[2]["model"], "gpt-4o-mini");
    let has_caveman = |b: &Value| {
        b["messages"][0]["content"]
            .as_str()
            .is_some_and(|t| t.contains("Token saver"))
    };
    assert!(has_caveman(&sent[0]) && has_caveman(&sent[1]));
    assert!(!has_caveman(&sent[2]), "per-request off");
    let roles: Vec<String> = h.usage.rows().iter().map(|r| r.role.clone()).collect();
    assert_eq!(roles, ["review", "chat", "summary"]);
}

#[tokio::test]
async fn rtk_compresses_tool_results_and_usage_reports_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("ok"))
        .mount(&server)
        .await;
    let h = harness(
        vec![conn(
            "o",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let grep = (0..40).fold(String::new(), |mut s, i| {
        let _ = writeln!(s, "src/lib.rs:{i}:    let x_{i} = compute();");
        s
    });
    let body = json!({"model": "openai/gpt-4o", "messages": [
        {"role": "user", "content": "find x"},
        {"role": "assistant", "content": null, "tool_calls": [{"id": "c", "type": "function", "function": {"name": "search", "arguments": "{}"}}]},
        {"role": "tool", "tool_call_id": "c", "content": grep}
    ]});
    run(&h.gateway, body).await.0.expect("ok");
    let sent = &bodies(&server).await[0];
    assert!(sent["messages"][2]["content"]
        .as_str()
        .is_some_and(|t| t.starts_with("40 matches in 1 files")));
    let row = &h.usage.rows()[0];
    assert!(row.saver_bytes_after > 0 && row.saver_bytes_after < row.saver_bytes_before);
}

#[tokio::test]
async fn legacy_router_settings_become_a_connection() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer legacy-key"))
        .respond_with(openai_text("legacy ok"))
        .mount(&server)
        .await;
    let store = Store::open_in_memory().expect("store");
    let mut legacy = RouterSettings::default()
        .with_base_url(&format!("{}/v1", server.uri()))
        .expect("url");
    legacy.default_model = Some("openai/gpt-4o".into());
    legacy.save(&store).expect("save");
    let config = GatewayConfig::load(&store).expect("config");
    assert_eq!(
        config.role_target("chat").as_deref(),
        Some("9router/openai/gpt-4o")
    );
    let gateway = Gateway::new(
        config,
        Arc::new(MemoryKeys::with(&[(LEGACY_ROUTER_ID, "legacy-key")])),
    )
    .expect("gateway")
    .with_state(Arc::new(GatewayState::default()));
    let (result, _) = run(&gateway, request("")).await;
    assert_eq!(result.expect("ok").content, "legacy ok");
    assert_eq!(bodies(&server).await[0]["model"], "openai/gpt-4o");
}

#[tokio::test]
async fn config_loads_rows_and_reports_unknown_models() {
    let store = Store::open_in_memory().expect("store");
    store
        .upsert_connection(&crate::store::ConnectionRow {
            id: "c1".into(),
            provider: "openai".into(),
            auth_type: "api_key".into(),
            name: "Work".into(),
            priority: 1,
            is_active: true,
            data: "{}".into(),
        })
        .expect("conn");
    store
        .upsert_connection(&crate::store::ConnectionRow {
            id: "c2".into(),
            provider: "anthropic".into(),
            auth_type: "api_key".into(),
            name: "Claude".into(),
            priority: 2,
            is_active: true,
            data: "{}".into(),
        })
        .expect("conn 2");
    store
        .upsert_combo(&ComboRow {
            id: "k".into(),
            name: "best".into(),
            kind: "llm".into(),
            models: r#"["openai/gpt-4o","nobody/x"]"#.into(),
            strategy: None,
            sticky_limit: None,
        })
        .expect("combo");
    let settings = GatewaySettings {
        default_model: Some("best".into()),
        ..GatewaySettings::default()
    };
    settings.save(&store).expect("save");
    let config = GatewayConfig::load(&store).expect("load");
    assert_eq!(GatewaySettings::load(&store).expect("settings"), settings);
    let plan = config.resolve("", "design").expect("plan");
    assert_eq!(
        plan.targets,
        [Target {
            provider: "openai".into(),
            model: "gpt-4o".into()
        }]
    );
    assert!(
        matches!(
            config.resolve("gpt-4o-mini", "chat"),
            Err(LlmError::Config(_))
        ),
        "bare model is ambiguous with two connections"
    );
    assert!(matches!(
        config.resolve("nobody/x", "chat"),
        Err(LlmError::Config(_))
    ));
    assert_eq!(
        config.accounts("Claude")[0].id,
        "c2",
        "connections answer to their name"
    );
    assert!(matches!(
        GatewayConfig::default().resolve("", "chat"),
        Err(LlmError::Config(_))
    ));
}

#[tokio::test]
async fn health_reflects_the_default_provider() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "key-an"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "claude-x"}]})),
        )
        .mount(&server)
        .await;
    let settings = GatewaySettings {
        default_model: Some("anthropic/claude-x".into()),
        ..GatewaySettings::default()
    };
    let h = harness(
        vec![conn(
            "an",
            "anthropic",
            ProviderKind::Anthropic,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        settings.clone(),
    );
    let report = h.gateway.health().await;
    assert_eq!(report.status, HealthStatus::Ok, "{}", report.detail);
    assert!(report.detail.contains("claude-x"));
    let account = h.gateway.config().connections[0].clone();
    assert_eq!(
        h.gateway.list_models(&account).await.expect("models"),
        ["claude-x"]
    );

    let down = harness(
        vec![conn(
            "an",
            "anthropic",
            ProviderKind::Anthropic,
            "http://127.0.0.1:9/v1",
            1,
        )],
        vec![],
        settings,
    );
    assert_eq!(down.gateway.health().await.status, HealthStatus::Down);
    let empty = harness(vec![], vec![], GatewaySettings::default());
    let report = empty.gateway.health().await;
    assert_eq!(report.status, HealthStatus::Down);
    assert!(report.detail.contains("Settings → Providers"));
}

#[test]
fn completion_client_works_from_sync_code() {
    let h = harness(vec![], vec![], GatewaySettings::default());
    let request = ChatRequest {
        model: String::new(),
        messages: vec![ChatMessage::user("hi")],
        tools: vec![],
    };
    assert!(matches!(
        h.gateway.complete(&request, &mut |_| {}),
        Err(LlmError::Config(_))
    ));
}
