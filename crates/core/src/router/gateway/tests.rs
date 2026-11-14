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
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
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
        confidence_threshold: smart::DEFAULT_THRESHOLD,
        weights: Vec::new(),
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
        caveman: Some(SaverLevel::Full),
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
                cache: true,
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
            confidence_threshold: None,
            weights: None,
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

// ---- smart combos ---------------------------------------------------------------

fn openai_finish(text: &str, finish: &str) -> ResponseTemplate {
    sse(&[
        json!({"choices":[{"delta":{"role":"assistant","content":text}}]}),
        json!({"choices":[{"delta":{},"finish_reason":finish}]}),
    ])
}

/// Two providers, one model each, behind a `smart` combo; usage and sticky
/// choices go to an in-memory database.
fn smart_gateway(
    alpha: &MockServer,
    beta: &MockServer,
    threshold: f64,
    store: &Arc<StoreUsage>,
) -> Gateway {
    let mut smart_combo = combo("smart", &["alpha/m1", "beta/m2"], Some(Strategy::Smart));
    smart_combo.confidence_threshold = threshold;
    let config = GatewayConfig {
        connections: vec![
            conn(
                "a",
                "alpha",
                ProviderKind::OpenAi,
                &format!("{}/v1", alpha.uri()),
                1,
            ),
            conn(
                "b",
                "beta",
                ProviderKind::OpenAi,
                &format!("{}/v1", beta.uri()),
                1,
            ),
        ],
        combos: vec![smart_combo],
        settings: GatewaySettings::default(),
        pricing: Pricing::builtin(),
        legacy_model: None,
    };
    Gateway::new(
        config,
        Arc::new(MemoryKeys::with(&[("a", "ka"), ("b", "kb")])),
    )
    .expect("gateway")
    .with_state(Arc::new(GatewayState::new(CooldownPolicy {
        base: Duration::from_millis(1),
        max: Duration::from_millis(2),
    })))
    .with_usage(store.clone())
}

async fn hits(server: &MockServer) -> usize {
    server.received_requests().await.unwrap_or_default().len()
}

#[tokio::test]
async fn smart_combo_sticks_per_session_fails_over_and_persists() {
    let (alpha, beta) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(openai_text("from alpha"))
        .up_to_n_times(1)
        .mount(&alpha)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&alpha)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("from beta"))
        .mount(&beta)
        .await;
    let store = Arc::new(StoreUsage::new(Store::open_in_memory().expect("db")));
    let task = smart_gateway(&alpha, &beta, 0.8, &store).with_session("t1");

    let (first, _) = run(&task, request("smart")).await;
    assert_eq!(first.expect("first").content, "from alpha");
    let (second, _) = run(&task, request("smart")).await;
    assert_eq!(second.expect("failover").content, "from beta");
    // Alpha is the combo's first model, but the session now sticks to beta.
    let (third, _) = run(&task, request("smart")).await;
    assert_eq!(third.expect("sticky").content, "from beta");
    assert_eq!(hits(&alpha).await, 2, "third call skipped alpha");

    // Another session starts from the top of the combo.
    let other = task.clone().with_session("t2");
    run(&other, request("smart")).await.0.expect("t2");
    assert_eq!(hits(&alpha).await, 3);

    // A fresh gateway (new process state) reads the sticky choice back, as a
    // subagent or compaction call of the same task would.
    let restarted = smart_gateway(&alpha, &beta, 0.8, &store).with_session("t1");
    run(&restarted, request("smart"))
        .await
        .0
        .expect("restarted");
    assert_eq!(hits(&alpha).await, 3, "still on beta after restart");

    let db = store.store();
    let sticky = db.get_sticky("t1", "smart").expect("read").expect("row");
    assert_eq!(sticky.model, "beta/m2");
    let switches = db.usage_summary(1, 20).expect("summary").switches;
    let reasons: Vec<(&str, &str, &str, &str)> = switches
        .iter()
        .map(|s| {
            (
                s.session.as_str(),
                s.from_model.as_str(),
                s.to_model.as_str(),
                s.reason.as_str(),
            )
        })
        .collect();
    // t2's first answer came from beta, but it had no sticky model to switch from.
    assert_eq!(reasons, [("t1", "alpha/m1", "beta/m2", "failover")]);
    assert_eq!(
        db.get_sticky("t2", "smart").expect("read").map(|s| s.model),
        Some("beta/m2".into())
    );
}

#[tokio::test]
async fn smart_combo_switches_when_quality_drops_below_the_threshold() {
    let (alpha, beta) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(openai_finish("cut off", "length"))
        .mount(&alpha)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("complete"))
        .mount(&beta)
        .await;
    let store = Arc::new(StoreUsage::new(Store::open_in_memory().expect("db")));
    let gateway = smart_gateway(&alpha, &beta, 0.9, &store).with_session("task-9");
    let ask = || {
        let gateway = gateway.clone();
        async move {
            gateway
                .chat_value(request("smart"), &RequestOptions::default(), &mut |_| {})
                .await
                .expect("answer")
        }
    };
    // Truncated answers are still returned; the first one is tolerated.
    assert_eq!(ask().await.content, "cut off");
    assert_eq!(ask().await.content, "cut off");
    // Confidence 0.8 < 0.9 after the second: the next request uses beta.
    assert_eq!(ask().await.content, "complete");
    assert_eq!(hits(&alpha).await, 2);

    let db = store.store();
    let switches = db.usage_summary(1, 20).expect("summary").switches;
    assert_eq!(switches.len(), 1);
    assert_eq!(switches[0].session, "task-9");
    assert_eq!(switches[0].reason, "quality:truncated");
    assert!((switches[0].confidence - 0.8).abs() < 1e-9);
    assert_eq!(db.usage_summary(1, 20).expect("usage").totals.requests, 3);
}

// ---- OmniRoute / OpenRouter features ---------------------------------------------

/// Prices and context windows for the fake providers `alpha`, `beta`, `gamma`.
fn test_pricing() -> Pricing {
    Pricing::builtin().with_override(
        Pricing::from_json(
            r#"{"providers":{
                "alpha":{"m":{"input":5,"output":5,"context_length":8000}},
                "beta":{"m":{"input":1,"output":1,"context_length":1000000}},
                "gamma":{"m":{"input":0.5,"output":0.5,"context_length":32000}}
            }}"#,
        )
        .expect("pricing"),
    )
}

/// A gateway on an in-memory database with fresh routing state.
fn db_gateway(
    connections: Vec<Connection>,
    combos: Vec<Combo>,
    settings: GatewaySettings,
) -> (Gateway, Arc<StoreUsage>) {
    let keys: Vec<(String, String)> = connections
        .iter()
        .map(|c| (c.id.clone(), format!("key-{}", c.id)))
        .collect();
    let refs: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let config = GatewayConfig {
        connections,
        combos,
        settings,
        pricing: test_pricing(),
        legacy_model: None,
    };
    let store = Arc::new(StoreUsage::new(Store::open_in_memory().expect("db")));
    let state = GatewayState::new(CooldownPolicy {
        base: Duration::from_millis(1),
        max: Duration::from_millis(2),
    })
    .with_breaker_policy(breaker::BreakerPolicy {
        threshold: 3,
        base: Duration::from_millis(60),
        max: Duration::from_millis(240),
    });
    let gateway = Gateway::new(config, Arc::new(MemoryKeys::with(&refs)))
        .expect("gateway")
        .with_state(Arc::new(state))
        .with_usage(store.clone());
    (gateway, store)
}

/// One server per fake provider, each answering with its own name.
async fn providers(names: &[&str]) -> (Vec<MockServer>, Vec<Connection>) {
    let mut servers = Vec::new();
    let mut connections = Vec::new();
    for name in names {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(openai_text(name))
            .mount(&server)
            .await;
        connections.push(conn(
            name,
            name,
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        ));
        servers.push(server);
    }
    (servers, connections)
}

async fn answer(gateway: &Gateway, body: Value) -> String {
    run(gateway, body).await.0.expect("answer").content
}

#[tokio::test]
async fn response_cache_replays_hits_and_skips_warm_or_opted_out_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("cached answer"))
        .mount(&server)
        .await;
    let settings = GatewaySettings {
        response_cache: ResponseCacheSettings {
            enabled: true,
            ..ResponseCacheSettings::default()
        },
        ..GatewaySettings::default()
    };
    let (gateway, store) = db_gateway(
        vec![conn(
            "o1",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        settings,
    );
    let cold = || {
        let mut body = request("openai/gpt-4o");
        body["temperature"] = json!(0);
        body
    };
    let (first, streamed_first) = run(&gateway, cold()).await;
    let (second, streamed_second) = run(&gateway, cold()).await;
    let (first, second) = (first.expect("miss"), second.expect("hit"));
    assert_eq!(
        (&first.content, &first.tool_calls, &first.finish_reason),
        (&second.content, &second.tool_calls, &second.finish_reason)
    );
    // The provider measured the miss; a cache hit consumed no tokens.
    assert!(first.usage.is_some());
    assert_eq!(second.usage, None);
    assert_eq!(streamed_first, streamed_second, "the stream is replayed");
    assert_eq!(hits(&server).await, 1, "second answer came from the cache");

    let mut warm = request("openai/gpt-4o");
    warm["temperature"] = json!(0.7);
    answer(&gateway, warm).await;
    assert_eq!(hits(&server).await, 2, "non-zero temperature is not cached");
    answer(&gateway, request("openai/gpt-4o")).await;
    assert_eq!(
        hits(&server).await,
        3,
        "no temperature = provider default, not cached"
    );
    gateway
        .chat_value(
            cold(),
            &RequestOptions {
                cache: false,
                ..RequestOptions::default()
            },
            &mut |_| {},
        )
        .await
        .expect("bypass");
    assert_eq!(hits(&server).await, 4, "per-request opt-out");

    let db = store.store();
    let summary = db.usage_summary(1, 10).expect("summary");
    assert_eq!(summary.cache_hits, 1);
    assert_eq!(summary.cache_saved_tokens, 1100);
    assert!(summary.cache_saved_cost > 0.0);
    assert_eq!(summary.totals.requests, 4, "hits are not provider requests");
    assert!(db.cache_clear().expect("clear") >= 1);
}

#[tokio::test]
async fn circuit_breaker_opens_after_three_failures_and_one_probe_closes_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("down"))
        .up_to_n_times(3)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(openai_text("back"))
        .mount(&server)
        .await;
    let (gateway, _) = db_gateway(
        vec![conn(
            "o1",
            "openai",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings {
            default_model: Some("openai/gpt-4o".into()),
            ..GatewaySettings::default()
        },
    );
    for _ in 0..3 {
        assert!(run(&gateway, request("openai/gpt-4o")).await.0.is_err());
    }
    let refused = run(&gateway, request("openai/gpt-4o")).await.0.unwrap_err();
    assert!(refused.to_string().contains("circuit open"), "{refused}");
    assert_eq!(hits(&server).await, 3, "open breaker sends nothing");
    let health = gateway.health().await;
    assert_eq!(health.status, HealthStatus::Down);
    assert!(health.detail.contains("circuit open"));
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        answer(&gateway, request("openai/gpt-4o")).await,
        "back",
        "probe succeeds"
    );
    assert_eq!(
        gateway.state().breakers.state("openai"),
        breaker::BreakerState::Closed
    );
}

#[tokio::test]
async fn breaker_health_moves_failing_providers_behind_healthy_ones() {
    let (servers, connections) = providers(&["alpha", "beta"]).await;
    let (gateway, _) = db_gateway(
        connections,
        vec![combo("pair", &["alpha/m", "beta/m"], None)],
        GatewaySettings::default(),
    );
    for _ in 0..3 {
        gateway.state().breakers.failure("alpha");
    }
    assert_eq!(answer(&gateway, request("pair")).await, "beta");
    assert_eq!(hits(&servers[0]).await, 0);
}

#[tokio::test]
async fn prompt_cache_affinity_keeps_a_conversation_on_one_account() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("ok"))
        .mount(&server)
        .await;
    let base = format!("{}/v1", server.uri());
    let (gateway, _) = db_gateway(
        vec![
            conn("k1", "openai", ProviderKind::OpenAi, &base, 1),
            conn("k2", "openai", ProviderKind::OpenAi, &base, 1),
        ],
        vec![],
        GatewaySettings::default(),
    );
    let conversation = |task: usize, turns: usize| {
        let mut messages = vec![
            json!({"role": "system", "content": "agent"}),
            json!({"role": "user", "content": format!("task {task}")}),
        ];
        for turn in 0..turns {
            messages.push(json!({"role": "assistant", "content": format!("step {turn}")}));
            messages.push(json!({"role": "user", "content": "go on"}));
        }
        json!({"model": "openai/gpt-4o", "messages": messages})
    };
    for turns in 0..4 {
        answer(&gateway, conversation(1, turns)).await;
    }
    for task in 2..14 {
        answer(&gateway, conversation(task, 0)).await;
    }
    let keys: Vec<String> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| {
            r.headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert!(
        keys[..4].iter().all(|k| *k == keys[0]),
        "one conversation, one account: {keys:?}"
    );
    let distinct: std::collections::HashSet<&String> = keys.iter().collect();
    assert_eq!(distinct.len(), 2, "conversations spread over both accounts");
}

#[tokio::test]
async fn combo_orderings_pick_by_cost_context_last_good_and_weight() {
    let (servers, connections) = providers(&["alpha", "beta", "gamma"]).await;
    let models = ["alpha/m", "beta/m", "gamma/m"];
    // `nobody/x` has no connection and is dropped; weights still follow models.
    let mut weighted = combo(
        "weighted",
        &["nobody/x", "alpha/m", "beta/m", "gamma/m"],
        Some(Strategy::Weighted),
    );
    weighted.weights = vec![9, 0, 1, 0];
    let (gateway, _) = db_gateway(
        connections,
        vec![
            combo("cheap", &models, Some(Strategy::CostOptimized)),
            combo("wide", &models, Some(Strategy::ContextOptimized)),
            combo("lkgp", &models, Some(Strategy::Lkgp)),
            weighted,
        ],
        GatewaySettings::default(),
    );
    assert_eq!(
        answer(&gateway, request("cheap")).await,
        "gamma",
        "$0.5 beats $1 and $5"
    );
    assert_eq!(
        answer(&gateway, request("wide")).await,
        "beta",
        "1M context first"
    );
    for _ in 0..5 {
        assert_eq!(
            answer(&gateway, request("weighted")).await,
            "beta",
            "only beta has weight"
        );
    }
    // lkgp: alpha answers first; once beta is the last good one, it leads.
    assert_eq!(answer(&gateway, request("lkgp")).await, "alpha");
    gateway.state().set_last_good("lkgp", "beta/m");
    assert_eq!(answer(&gateway, request("lkgp")).await, "beta");
    assert_eq!(hits(&servers[0]).await, 1);
}

#[tokio::test]
async fn lkgp_remembers_the_model_that_answered_after_a_failure() {
    let alpha = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&alpha)
        .await;
    let beta = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("beta"))
        .mount(&beta)
        .await;
    let (gateway, _) = db_gateway(
        vec![
            conn(
                "alpha",
                "alpha",
                ProviderKind::OpenAi,
                &format!("{}/v1", alpha.uri()),
                1,
            ),
            conn(
                "beta",
                "beta",
                ProviderKind::OpenAi,
                &format!("{}/v1", beta.uri()),
                1,
            ),
        ],
        vec![combo("lkgp", &["alpha/m", "beta/m"], Some(Strategy::Lkgp))],
        GatewaySettings::default(),
    );
    assert_eq!(answer(&gateway, request("lkgp")).await, "beta");
    assert_eq!(answer(&gateway, request("lkgp")).await, "beta");
    assert_eq!(
        hits(&alpha).await,
        1,
        "second request went straight to beta"
    );
}

#[tokio::test]
async fn context_overflow_moves_to_a_model_that_fits() {
    let (servers, connections) = providers(&["alpha", "beta"]).await;
    let (gateway, _) = db_gateway(
        connections,
        vec![combo("pair", &["alpha/m", "beta/m"], None)],
        GatewaySettings::default(),
    );
    assert_eq!(
        answer(&gateway, request("pair")).await,
        "alpha",
        "small prompt: combo order"
    );
    let big =
        json!({"model": "pair", "messages": [{"role": "user", "content": "x".repeat(100_000)}]});
    assert_eq!(
        answer(&gateway, big.clone()).await,
        "beta",
        "25k tokens > alpha's 8k"
    );
    let sent = bodies(&servers[1]).await;
    assert_eq!(sent[0]["messages"], big["messages"], "nothing trimmed");
}

fn record_spend(store: &StoreUsage, cost: f64) {
    store
        .store()
        .record_usage(&UsageRow {
            provider: "alpha".into(),
            model: "m".into(),
            role: "chat".into(),
            status: 200,
            cost,
            ..UsageRow::default()
        })
        .expect("usage");
}

#[tokio::test]
async fn budgets_warn_then_block_or_fall_back_to_the_cheapest_model() {
    let (servers, connections) = providers(&["alpha", "gamma"]).await;
    let budget = |action| GatewaySettings {
        budgets: vec![Budget {
            role: Some("chat".into()),
            period: Period::Daily,
            limit_usd: 1.0,
        }],
        budget_action: action,
        ..GatewaySettings::default()
    };
    let combos = vec![combo("pair", &["alpha/m", "gamma/m"], None)];
    let (blocking, store) = db_gateway(
        connections.clone(),
        combos.clone(),
        budget(BudgetAction::Block),
    );
    record_spend(&store, 0.85);
    assert_eq!(blocking.budget_status()[0].level, Level::Warn);
    assert_eq!(
        answer(&blocking, request("pair")).await,
        "alpha",
        "80% only warns"
    );
    record_spend(&store, 0.5);
    let error = run(&blocking, request("pair")).await.0.unwrap_err();
    assert!(matches!(error, LlmError::Http { status: 402, .. }));
    assert!(
        error.to_string().contains("Daily budget for chat reached"),
        "{error}"
    );
    let review = blocking.clone().with_role("review");
    assert_eq!(
        answer(&review, request("pair")).await,
        "alpha",
        "other roles are not capped"
    );

    let (cheapest, store) = db_gateway(connections, combos, budget(BudgetAction::Cheapest));
    record_spend(&store, 2.0);
    assert_eq!(
        answer(&cheapest, request("pair")).await,
        "gamma",
        "cheapest model only"
    );
    assert_eq!(hits(&servers[1]).await, 1);
}

#[tokio::test]
async fn reasoning_setting_maps_per_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(openai_text("o"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(anthropic_text("a"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/models/gemini-2.5-flash:streamGenerateContent"))
        .respond_with(gemini_text("g"))
        .mount(&server)
        .await;
    let base = format!("{}/v1", server.uri());
    let mut settings = GatewaySettings::default();
    settings.reasoning.insert(
        "chat".into(),
        ReasoningSetting {
            effort: reasoning::Effort::High,
            max_tokens: None,
        },
    );
    let (gateway, _) = db_gateway(
        vec![
            conn("o", "openai", ProviderKind::OpenAi, &base, 1),
            conn("a", "anthropic", ProviderKind::Anthropic, &base, 1),
            conn("g", "gemini", ProviderKind::Gemini, &base, 1),
        ],
        vec![],
        settings,
    );
    answer(&gateway, request("openai/gpt-5")).await;
    answer(&gateway, request("anthropic/claude-sonnet-4-5")).await;
    answer(&gateway, request("gemini/gemini-2.5-flash")).await;
    let sent = bodies(&server).await;
    assert_eq!(sent[0]["reasoning_effort"], "high");
    assert!(sent[0].get("reasoning").is_none());
    assert_eq!(
        sent[1]["thinking"],
        json!({"type": "enabled", "budget_tokens": 51_200})
    );
    assert_eq!(sent[1]["max_tokens"], 64_000);
    assert_eq!(
        sent[2]["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 32_768})
    );
}

#[tokio::test]
async fn anthropic_thinking_blocks_go_back_unchanged_during_tool_use() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(sse(&[
            json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"I should read it"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIG-1"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a\"}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
            json!({"type":"message_stop"}),
        ]))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(anthropic_text("done"))
        .mount(&server)
        .await;
    let (gateway, _) = db_gateway(
        vec![conn(
            "a",
            "anthropic",
            ProviderKind::Anthropic,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    let first = answer_completion(&gateway, request("anthropic/claude-sonnet-4-5")).await;
    assert_eq!(first.tool_calls[0].id, "toolu_1");
    // The agent sends back only text and tool calls, as it does today.
    let follow_up = json!({"model": "anthropic/claude-sonnet-4-5", "messages": [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": null, "tool_calls": [
            {"id": "toolu_1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a\"}"}}
        ]},
        {"role": "tool", "tool_call_id": "toolu_1", "content": "file text"}
    ]});
    answer(&gateway, follow_up).await;
    let sent = bodies(&server).await;
    assert_eq!(
        sent[1]["messages"][1]["content"][0],
        json!({"type": "thinking", "thinking": "I should read it", "signature": "SIG-1"})
    );
    assert_eq!(sent[1]["messages"][1]["content"][1]["type"], "tool_use");
}

async fn answer_completion(gateway: &Gateway, body: Value) -> Completion {
    run(gateway, body).await.0.expect("answer")
}

#[tokio::test]
async fn cache_reads_are_priced_with_the_provider_multiplier() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_text("hi"))
        .mount(&server)
        .await;
    let (gateway, store) = db_gateway(
        vec![conn(
            "d",
            "deepseek",
            ProviderKind::OpenAi,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        GatewaySettings::default(),
    );
    answer(&gateway, request("deepseek/mystery-chat")).await;
    // No price for the model: unknown, $0.
    let row = &store.store().usage_summary(1, 5).expect("summary").recent[0];
    assert!(!row.cost_known);
    let priced = Pricing::from_json(r#"{"models":{"mystery-chat":{"input":1,"output":2}},"cache_multipliers":{"deepseek":{"read":0.1}}}"#)
        .expect("pricing");
    // openai_text reports 1000 prompt (200 cached) + 100 completion tokens.
    let usage = Usage {
        prompt: 1000,
        completion: 100,
        cached: 200,
        cache_creation: 0,
        reasoning: 0,
    };
    let expected = (800.0 * 1.0 + 200.0 * 0.1 + 100.0 * 2.0) / 1e6;
    assert!((priced.cost("deepseek", "mystery-chat", &usage).usd - expected).abs() < 1e-12);
}

// ---- security review regressions ---------------------------------------------------

fn cache_on() -> GatewaySettings {
    GatewaySettings {
        response_cache: ResponseCacheSettings {
            enabled: true,
            ..ResponseCacheSettings::default()
        },
        ..GatewaySettings::default()
    }
}

fn cold(model: &str) -> Value {
    let mut body = request(model);
    body["temperature"] = json!(0);
    body
}

/// H1: truncated, filtered and unterminated answers are never cached.
#[tokio::test]
async fn incomplete_answers_are_not_cached() {
    for (label, template) in [
        ("length", openai_finish("cut", "length")),
        ("content_filter", openai_finish("hidden", "content_filter")),
        (
            "no finish, no [DONE]",
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"half\"}}]}\n\n"),
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .mount(&server)
            .await;
        let (gateway, store) = db_gateway(
            vec![conn(
                "o1",
                "openai",
                ProviderKind::OpenAi,
                &format!("{}/v1", server.uri()),
                1,
            )],
            vec![],
            cache_on(),
        );
        run(&gateway, cold("openai/gpt-4o")).await.0.expect(label);
        run(&gateway, cold("openai/gpt-4o")).await.0.expect(label);
        assert_eq!(hits(&server).await, 2, "{label}: nothing was cached");
        assert_eq!(store.store().cache_count().expect("count"), 0, "{label}");
    }
}

/// H2: two endpoints of the same provider never share cache entries.
#[tokio::test]
async fn custom_endpoints_do_not_share_cache_entries() {
    let (one, two) = (MockServer::start().await, MockServer::start().await);
    for (server, text) in [(&one, "from one"), (&two, "from two")] {
        Mock::given(method("POST"))
            .respond_with(openai_text(text))
            .mount(server)
            .await;
    }
    let (gateway, _) = db_gateway(
        vec![
            conn(
                "c1",
                "custom",
                ProviderKind::OpenAi,
                &format!("{}/v1", one.uri()),
                1,
            ),
            conn(
                "c2",
                "custom",
                ProviderKind::OpenAi,
                &format!("{}/v1", two.uri()),
                1,
            ),
        ],
        vec![],
        cache_on(),
    );
    assert_eq!(answer(&gateway, cold("c1/m")).await, "from one");
    assert_eq!(
        answer(&gateway, cold("c2/m")).await,
        "from two",
        "not one's cached answer"
    );
    assert_eq!(answer(&gateway, cold("c1/m")).await, "from one");
    assert_eq!(
        (hits(&one).await, hits(&two).await),
        (1, 1),
        "each endpoint cached its own"
    );
}

/// M3: a cache hit restores the reasoning blocks of its tool calls.
#[tokio::test]
async fn cache_hits_bring_back_reasoning_details() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"temperature": 0})))
        .respond_with(sse(&[
            json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIG-C"}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_c","name":"read_file"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ]))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(anthropic_text("done"))
        .mount(&server)
        .await;
    let (first, _) = db_gateway(
        vec![conn(
            "a",
            "anthropic",
            ProviderKind::Anthropic,
            &format!("{}/v1", server.uri()),
            1,
        )],
        vec![],
        cache_on(),
    );
    answer_completion(&first, cold("anthropic/claude-sonnet-4-5")).await;
    // A fresh process: no reasoning memory, same database.
    let fresh = first.clone().with_state(Arc::new(GatewayState::default()));
    let hit = answer_completion(&fresh, cold("anthropic/claude-sonnet-4-5")).await;
    assert_eq!(hits(&server).await, 1, "served from the cache");
    assert_eq!(hit.tool_calls[0].id, "toolu_c");
    answer(
        &fresh,
        json!({"model": "anthropic/claude-sonnet-4-5", "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "toolu_c", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}
            ]},
            {"role": "tool", "tool_call_id": "toolu_c", "content": "text"}
        ]}),
    )
    .await;
    let sent = bodies(&server).await;
    assert_eq!(
        sent[1]["messages"][1]["content"][0],
        json!({"type": "thinking", "thinking": "plan", "signature": "SIG-C"})
    );
}

fn capped(refuse_unpriced: bool) -> GatewaySettings {
    GatewaySettings {
        budgets: vec![Budget {
            role: None,
            period: Period::Daily,
            limit_usd: 1.0,
        }],
        budget_refuse_unpriced: refuse_unpriced,
        ..GatewaySettings::default()
    }
}

/// M1: unpriced models are refused while a cap applies (unless allowed).
#[tokio::test]
async fn budgets_refuse_unpriced_models_by_default() {
    let (_servers, connections) = providers(&["alpha"]).await;
    let (strict, _) = db_gateway(connections.clone(), vec![], capped(true));
    let error = run(&strict, request("alpha/no-price")).await.0.unwrap_err();
    assert!(
        matches!(error, LlmError::Http { status: 402, .. }),
        "{error}"
    );
    assert!(error.to_string().contains("known price"), "{error}");
    assert_eq!(
        answer(&strict, request("alpha/m")).await,
        "alpha",
        "priced models still work"
    );
    let (lenient, _) = db_gateway(connections.clone(), vec![], capped(false));
    assert_eq!(answer(&lenient, request("alpha/no-price")).await, "alpha");
    let (uncapped, _) = db_gateway(connections, vec![], GatewaySettings::default());
    assert_eq!(
        answer(&uncapped, request("alpha/no-price")).await,
        "alpha",
        "no cap, no check"
    );
}

/// M1: in-flight reservations count against the cap.
#[tokio::test]
async fn budget_reservations_stop_concurrent_requests_slipping_under() {
    let (servers, connections) = providers(&["alpha"]).await;
    let (gateway, _) = db_gateway(connections, vec![], capped(true));
    let held = gateway.state().reserve("chat", 0.999);
    let error = run(&gateway, request("alpha/m")).await.0.unwrap_err();
    assert!(
        matches!(error, LlmError::Http { status: 402, .. }),
        "{error}"
    );
    assert_eq!(hits(&servers[0]).await, 0);
    drop(held);
    assert_eq!(gateway.state().reserved(None), 0.0, "released");
    assert_eq!(answer(&gateway, request("alpha/m")).await, "alpha");
    assert_eq!(
        gateway.state().reserved(None),
        0.0,
        "released after the request"
    );
}

struct BrokenSpend;

impl UsageSink for BrokenSpend {
    fn record(&self, _row: &UsageRow) {}
    fn spend(&self, _role: Option<&str>, _monthly: bool) -> Result<Option<f64>, String> {
        Err("database is locked".into())
    }
}

/// M1: when spend cannot be read, budgets fail closed.
#[tokio::test]
async fn budgets_fail_closed_when_spend_cannot_be_read() {
    let (servers, connections) = providers(&["alpha"]).await;
    let (gateway, _) = db_gateway(connections, vec![], capped(true));
    let broken = gateway.with_usage(Arc::new(BrokenSpend));
    let error = run(&broken, request("alpha/m")).await.0.unwrap_err();
    assert!(error.to_string().contains("could not be read"), "{error}");
    assert_eq!(hits(&servers[0]).await, 0);
}
