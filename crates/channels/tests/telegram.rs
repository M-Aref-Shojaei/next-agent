//! The Telegram adapter against a mocked Bot API (wiremock).

use next_agent_channels::telegram::{test_token, TelegramChannel};
use next_agent_channels::{
    supervise, Backoff, Channel, ChannelConfig, ChannelError, ChatKind, HealthState, Outbound,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = "123456:ABCdefGHIjklMNOpqrSTUvwxYZ_0123";

fn api(method_name: &str) -> String {
    format!("/bot{TOKEN}/{method_name}")
}

#[allow(clippy::needless_pass_by_value)]
fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": result }))
}

async fn mock_me(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(api("getMe")))
        .respond_with(ok(
            json!({ "id": 999, "is_bot": true, "username": "NextBot", "first_name": "Next" }),
        ))
        .mount(server)
        .await;
}

fn channel(server: &MockServer) -> TelegramChannel {
    let config = ChannelConfig {
        id: "ch_1".into(),
        workspace_id: "ws".into(),
        ..ChannelConfig::default()
    };
    TelegramChannel::new(config, &server.uri(), TOKEN, Duration::from_secs(0)).expect("channel")
}

#[tokio::test]
async fn test_connection_returns_the_bot_identity() {
    let server = MockServer::start().await;
    mock_me(&server).await;
    let me = test_token(&server.uri(), TOKEN).await.expect("me");
    assert_eq!(me.username, "NextBot");
    assert_eq!(me.id, "999");
}

#[tokio::test]
async fn rejected_token_is_fatal_and_never_echoed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(api("getMe")))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "ok": false, "error_code": 401, "description": format!("Unauthorized {TOKEN}") })))
        .mount(&server)
        .await;
    let error = test_token(&server.uri(), TOKEN).await.expect_err("401");
    assert!(matches!(error, ChannelError::Fatal(_)));
    assert!(!error.message().contains("ABCdef"), "{error}");
    let bad = test_token(&server.uri(), "not-a-token")
        .await
        .expect_err("shape");
    assert!(matches!(bad, ChannelError::Fatal(_)));
}

#[tokio::test]
async fn unreachable_api_does_not_leak_the_token() {
    // Nothing listens on this port.
    let error = test_token("http://127.0.0.1:9", TOKEN)
        .await
        .expect_err("down");
    assert!(matches!(error, ChannelError::Transient { .. }));
    assert!(!error.message().contains("ABCdef"), "{error}");
}

#[tokio::test]
async fn long_polling_delivers_messages_and_advances_the_offset() {
    let server = MockServer::start().await;
    mock_me(&server).await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .and(body_partial_json(json!({ "offset": 0 })))
        .respond_with(ok(json!([
            { "update_id": 5, "message": { "message_id": 1, "text": "hi", "from": { "id": 42, "is_bot": false, "username": "alice" }, "chat": { "id": 42, "type": "private" } } },
            { "update_id": 6, "message": { "message_id": 2, "text": "from a bot", "from": { "id": 7, "is_bot": true }, "chat": { "id": 7, "type": "private" } } }
        ])))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .and(body_partial_json(json!({ "offset": 7 })))
        .respond_with(ok(json!([])).set_delay(Duration::from_millis(50)))
        .mount(&server)
        .await;
    let channel = std::sync::Arc::new(channel(&server));
    let (tx, mut rx) = mpsc::channel(8);
    let mut sup = supervise(channel, tx, Backoff::default());
    let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("in time")
        .expect("message");
    assert_eq!(message.text, "hi");
    assert_eq!(message.chat_kind, Some(ChatKind::Direct));
    assert_eq!(message.event_id, "5");
    loop {
        if sup.health.borrow().state == HealthState::Connected {
            break;
        }
        sup.health.changed().await.expect("health");
    }
    // The bot's own update was skipped; polling continues from offset 7.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let polls: Vec<Request> = server.received_requests().await.unwrap_or_default();
    assert!(polls.iter().any(|r| r.url.path().ends_with("getUpdates")
        && String::from_utf8_lossy(&r.body).contains("\"offset\":7")));
    assert!(rx.try_recv().is_err());
    sup.stop().await;
}

#[tokio::test]
async fn invalid_token_while_polling_marks_the_channel_failed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(api("getMe")))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            json!({ "ok": false, "error_code": 401, "description": "Unauthorized" }),
        ))
        .mount(&server)
        .await;
    let (tx, _rx) = mpsc::channel(8);
    let mut sup = supervise(
        std::sync::Arc::new(channel(&server)),
        tx,
        Backoff::default(),
    );
    loop {
        let state = sup.health.borrow().state;
        if state == HealthState::Failed {
            break;
        }
        sup.health.changed().await.expect("health");
    }
    assert!(sup.health.borrow().detail.contains("token"));
}

#[tokio::test]
async fn conflict_is_retried_not_fatal() {
    let server = MockServer::start().await;
    mock_me(&server).await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({ "ok": false, "error_code": 409, "description": "Conflict: terminated by other getUpdates request" })))
        .mount(&server)
        .await;
    let (tx, _rx) = mpsc::channel(8);
    let mut sup = supervise(
        std::sync::Arc::new(channel(&server)),
        tx,
        Backoff {
            initial: Duration::from_millis(10),
            ..Backoff::default()
        },
    );
    loop {
        let state = sup.health.borrow().state;
        if state == HealthState::Reconnecting {
            break;
        }
        sup.health.changed().await.expect("health");
    }
    assert!(sup.health.borrow().detail.contains("another program"));
    sup.stop().await;
}

#[tokio::test]
async fn send_edit_and_typing_use_the_bot_api() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(api("sendMessage")))
        .and(body_partial_json(json!({ "chat_id": -100, "text": "hello", "reply_parameters": { "message_id": 5 }, "message_thread_id": 3 })))
        .respond_with(ok(json!({ "message_id": 77 })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .and(body_partial_json(
            json!({ "chat_id": -100, "message_id": 77, "text": "hello world" }),
        ))
        .respond_with(ok(json!(true)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("sendChatAction")))
        .and(body_partial_json(
            json!({ "chat_id": -100, "action": "typing" }),
        ))
        .respond_with(ok(json!(true)))
        .expect(1)
        .mount(&server)
        .await;
    let channel = channel(&server);
    let sent = channel
        .send(Outbound {
            chat_id: "-100".into(),
            text: "hello".into(),
            reply_to: Some("5".into()),
            thread_id: Some("3".into()),
        })
        .await
        .expect("send");
    assert_eq!(sent.message_id, "77");
    channel
        .edit(sent, "hello world".into())
        .await
        .expect("edit");
    channel.typing("-100".into()).await.expect("typing");
}

#[tokio::test]
async fn unchanged_edit_is_not_an_error_and_rate_limits_are_transient() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "ok": false, "error_code": 400, "description": "Bad Request: message is not modified" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("sendMessage")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "ok": false, "error_code": 429, "description": "Too Many Requests", "parameters": { "retry_after": 3 } })))
        .mount(&server)
        .await;
    let channel = channel(&server);
    let target = next_agent_channels::MessageRef {
        chat_id: "1".into(),
        message_id: "2".into(),
    };
    channel
        .edit(target, "same".into())
        .await
        .expect("not modified is fine");
    let error = channel
        .send(Outbound {
            chat_id: "1".into(),
            text: "x".into(),
            ..Outbound::default()
        })
        .await
        .expect_err("429");
    assert_eq!(
        error,
        ChannelError::Transient {
            message: "Telegram rate limit (Too Many Requests)".into(),
            retry_after: Some(3)
        }
    );
}

#[tokio::test]
async fn idempotency_keys_are_scoped_to_the_bot() {
    let server = MockServer::start().await;
    let first = channel(&server);
    assert_eq!(first.key_scope(), "ch_1:123456");
    let config = ChannelConfig {
        id: "ch_1".into(),
        workspace_id: "ws".into(),
        ..ChannelConfig::default()
    };
    let replaced = TelegramChannel::new(
        config,
        &server.uri(),
        "777:ZYXwvuTSRqpoNMLkjiHGFedcba_0123",
        Duration::from_secs(0),
    )
    .expect("channel");
    assert_ne!(
        first.key_scope(),
        replaced.key_scope(),
        "a new bot must not reuse the old bot's keys"
    );
}
