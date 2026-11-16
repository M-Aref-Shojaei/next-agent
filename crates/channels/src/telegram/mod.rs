//! Telegram channel: long polling with `getUpdates`, plain-text replies
//! chunked at 4,096, a typing indicator, and edits for streaming previews.
//! Media is not supported yet (the hub answers with a clear message).
//!
//! Replaces `OpenClaw`'s grammY-based adapter (`extensions/telegram`) with a
//! small client of our own; no upstream code.

pub mod api;

use crate::channel::{BoxFuture, Channel, ChannelError, ConnectedHook};
use crate::chunk::TELEGRAM_TEXT_LIMIT;
use crate::config::ChannelConfig;
use crate::types::{BotIdentity, Capabilities, ChatKind, Inbound, MessageRef, Outbound};
use api::TelegramApi;
use serde_json::Value;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;

/// Long-poll timeout used by the app.
pub const POLL_TIMEOUT: Duration = Duration::from_secs(25);

const MEDIA_KINDS: &[&str] = &[
    "photo",
    "document",
    "voice",
    "audio",
    "video",
    "video_note",
    "sticker",
    "animation",
    "location",
    "contact",
];

/// One Telegram bot.
pub struct TelegramChannel {
    config: ChannelConfig,
    api: TelegramApi,
    me: Mutex<Option<BotIdentity>>,
}

impl TelegramChannel {
    /// # Errors
    /// Malformed token (see [`api::valid_token`]).
    pub fn new(
        config: ChannelConfig,
        base_url: &str,
        token: &str,
        poll: Duration,
    ) -> Result<Self, ChannelError> {
        Ok(Self {
            api: TelegramApi::new(base_url, token, poll)?.for_channel(&config.id),
            config,
            me: Mutex::new(None),
        })
    }

    async fn identity(&self) -> Result<BotIdentity, ChannelError> {
        if let Some(me) = self
            .me
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Ok(me);
        }
        let me = identity_from(&self.api.get_me().await?);
        *self.me.lock().unwrap_or_else(PoisonError::into_inner) = Some(me.clone());
        Ok(me)
    }
}

fn identity_from(value: &Value) -> BotIdentity {
    BotIdentity {
        id: value["id"]
            .as_i64()
            .map_or_else(String::new, |i| i.to_string()),
        username: value["username"].as_str().unwrap_or_default().to_string(),
        name: value["first_name"].as_str().unwrap_or_default().to_string(),
    }
}

/// Text covered by a UTF-16 `offset`/`length` entity.
fn entity_text(text: &str, offset: usize, length: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = (offset + length).min(units.len());
    String::from_utf16_lossy(units.get(offset..end).unwrap_or_default())
}

/// Turn one `getUpdates` entry into an [`Inbound`]. `None` for updates we
/// ignore (no message, bots, broadcast channels, service messages).
#[must_use]
pub fn parse_update(update: &Value, me: &BotIdentity) -> Option<Inbound> {
    let message = update.get("message")?;
    let from = message.get("from")?;
    if from["is_bot"].as_bool() == Some(true) {
        return None;
    }
    let chat = message.get("chat")?;
    let chat_kind = match chat["type"].as_str()? {
        "private" => ChatKind::Direct,
        "group" | "supergroup" => ChatKind::Group,
        _ => return None,
    };
    let text = message["text"]
        .as_str()
        .or_else(|| message["caption"].as_str())
        .unwrap_or_default()
        .to_string();
    let media = MEDIA_KINDS
        .iter()
        .find(|kind| message.get(**kind).is_some())
        .map(|kind| (*kind).to_string());
    if text.is_empty() && media.is_none() {
        return None;
    }
    let handle = format!("@{}", me.username).to_lowercase();
    let entities = message["entities"]
        .as_array()
        .or_else(|| message["caption_entities"].as_array());
    let mentioned = !me.username.is_empty()
        && entities.is_some_and(|list| {
            list.iter().any(|e| {
                let kind = e["type"].as_str().unwrap_or_default();
                let offset = usize::try_from(e["offset"].as_u64().unwrap_or(0)).unwrap_or(0);
                let length = usize::try_from(e["length"].as_u64().unwrap_or(0)).unwrap_or(0);
                let covered = entity_text(&text, offset, length).to_lowercase();
                (kind == "mention" && covered == handle)
                    || (kind == "bot_command" && covered.ends_with(&handle))
            })
        });
    let reply_to_bot = !me.id.is_empty()
        && message["reply_to_message"]["from"]["id"]
            .as_i64()
            .is_some_and(|id| id.to_string() == me.id);
    // Only a real username gets the `@`: a first name of `@owner` must not
    // pass for one.
    let sender_label = from["username"].as_str().map_or_else(
        || {
            from["first_name"]
                .as_str()
                .unwrap_or_default()
                .trim_start_matches(['@', ' '])
                .to_string()
        },
        |u| format!("@{u}"),
    );
    let thread_id = (message["is_topic_message"].as_bool() == Some(true))
        .then(|| message["message_thread_id"].as_i64().map(|t| t.to_string()))
        .flatten();
    Some(Inbound {
        event_id: update["update_id"].as_i64()?.to_string(),
        chat_id: chat["id"].as_i64()?.to_string(),
        chat_kind: Some(chat_kind),
        sender_id: from["id"].as_i64()?.to_string(),
        sender_label,
        message_id: message["message_id"].as_i64()?.to_string(),
        thread_id,
        text,
        mentioned,
        reply_to_bot,
        media,
    })
}

impl Channel for TelegramChannel {
    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn key_scope(&self) -> String {
        // The token's numeric prefix is the bot's id (not secret).
        let bot = self.api.bot_id();
        format!("{}:{bot}", self.config.id)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text_chunk_limit: TELEGRAM_TEXT_LIMIT,
            edit: true,
            typing: true,
            threads: true,
            media: false,
        }
    }

    fn probe(&self) -> BoxFuture<'_, Result<BotIdentity, ChannelError>> {
        Box::pin(async move {
            *self.me.lock().unwrap_or_else(PoisonError::into_inner) = None;
            self.identity().await
        })
    }

    fn start(
        &self,
        inbound: mpsc::Sender<Inbound>,
        connected: ConnectedHook,
    ) -> BoxFuture<'_, Result<(), ChannelError>> {
        Box::pin(async move {
            let me = self.identity().await?;
            let mut offset: i64 = 0;
            let mut announced = false;
            loop {
                let updates = self.api.get_updates(offset).await?;
                if !announced {
                    connected();
                    announced = true;
                }
                for update in updates {
                    if let Some(id) = update["update_id"].as_i64() {
                        offset = offset.max(id + 1);
                    }
                    if let Some(message) = parse_update(&update, &me) {
                        if inbound.send(message).await.is_err() {
                            return Ok(());
                        }
                    }
                }
                if inbound.is_closed() {
                    return Ok(());
                }
            }
        })
    }

    fn send(&self, message: Outbound) -> BoxFuture<'_, Result<MessageRef, ChannelError>> {
        Box::pin(async move {
            let id = self
                .api
                .send_message(
                    &message.chat_id,
                    &message.text,
                    message.reply_to.as_deref(),
                    message.thread_id.as_deref(),
                )
                .await?;
            Ok(MessageRef {
                chat_id: message.chat_id,
                message_id: id,
            })
        })
    }

    fn edit(&self, target: MessageRef, text: String) -> BoxFuture<'_, Result<(), ChannelError>> {
        Box::pin(async move {
            self.api
                .edit_message_text(&target.chat_id, &target.message_id, &text)
                .await
        })
    }

    fn typing(&self, chat_id: String) -> BoxFuture<'_, Result<(), ChannelError>> {
        Box::pin(async move { self.api.send_typing(&chat_id).await })
    }
}

/// "Test connection": check a token and return the bot's identity.
///
/// # Errors
/// Malformed or rejected token, or Telegram unreachable.
pub async fn test_token(base_url: &str, token: &str) -> Result<BotIdentity, ChannelError> {
    let api = TelegramApi::new(base_url, token, Duration::from_secs(1))?;
    Ok(identity_from(&api.get_me().await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn me() -> BotIdentity {
        BotIdentity {
            id: "999".into(),
            username: "NextBot".into(),
            name: "Next".into(),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn update(message: Value) -> Value {
        json!({ "update_id": 10, "message": message })
    }

    #[test]
    fn first_names_cannot_pass_for_usernames() {
        let u = update(json!({
            "message_id": 5, "text": "hi",
            "from": { "id": 7, "is_bot": false, "first_name": "@owner" },
            "chat": { "id": 7, "type": "private" }
        }));
        assert_eq!(parse_update(&u, &me()).expect("m").sender_label, "owner");
    }

    #[test]
    fn parses_a_private_text_message() {
        let u = update(json!({
            "message_id": 5, "text": "hello",
            "from": { "id": 42, "is_bot": false, "username": "alice", "first_name": "Alice" },
            "chat": { "id": 42, "type": "private" }
        }));
        let m = parse_update(&u, &me()).expect("message");
        assert_eq!(m.event_id, "10");
        assert_eq!(m.chat_kind, Some(ChatKind::Direct));
        assert_eq!(m.sender_id, "42");
        assert_eq!(m.sender_label, "@alice");
        assert_eq!(m.text, "hello");
        assert!(!m.mentioned && !m.reply_to_bot && m.media.is_none());
    }

    #[test]
    fn detects_mentions_with_utf16_offsets_and_replies_to_the_bot() {
        let text = "😀 @nextbot hi";
        let u = update(json!({
            "message_id": 6, "text": text,
            "entities": [{ "type": "mention", "offset": 3, "length": 8 }],
            "reply_to_message": { "message_id": 1, "from": { "id": 999, "is_bot": true } },
            "from": { "id": 42, "is_bot": false, "first_name": "Al" },
            "chat": { "id": -100, "type": "supergroup" }
        }));
        let m = parse_update(&u, &me()).expect("message");
        assert_eq!(m.chat_kind, Some(ChatKind::Group));
        assert!(m.mentioned);
        assert!(m.reply_to_bot);
        assert_eq!(m.sender_label, "Al");
    }

    #[test]
    fn other_mentions_and_commands_for_other_bots_do_not_count() {
        let u = update(json!({
            "message_id": 7, "text": "/stop@OtherBot @someone",
            "entities": [{ "type": "bot_command", "offset": 0, "length": 14 }, { "type": "mention", "offset": 15, "length": 8 }],
            "from": { "id": 42, "is_bot": false }, "chat": { "id": -100, "type": "group" }
        }));
        assert!(!parse_update(&u, &me()).expect("m").mentioned);
        let u = update(json!({
            "message_id": 8, "text": "/stop@NextBot",
            "entities": [{ "type": "bot_command", "offset": 0, "length": 13 }],
            "from": { "id": 42, "is_bot": false }, "chat": { "id": -100, "type": "group" }
        }));
        assert!(parse_update(&u, &me()).expect("m").mentioned);
    }

    #[test]
    fn media_is_flagged_and_bots_and_channels_are_ignored() {
        let photo = update(json!({
            "message_id": 9, "photo": [{}],
            "from": { "id": 42, "is_bot": false }, "chat": { "id": 42, "type": "private" }
        }));
        assert_eq!(
            parse_update(&photo, &me()).expect("m").media.as_deref(),
            Some("photo")
        );
        let bot = update(json!({
            "message_id": 9, "text": "x",
            "from": { "id": 7, "is_bot": true }, "chat": { "id": 7, "type": "private" }
        }));
        assert!(parse_update(&bot, &me()).is_none());
        let channel = update(json!({
            "message_id": 9, "text": "x",
            "from": { "id": 7, "is_bot": false }, "chat": { "id": -5, "type": "channel" }
        }));
        assert!(parse_update(&channel, &me()).is_none());
        assert!(parse_update(&json!({ "update_id": 1 }), &me()).is_none());
    }

    #[test]
    fn forum_topics_become_threads() {
        let u = update(json!({
            "message_id": 9, "text": "x", "is_topic_message": true, "message_thread_id": 77,
            "from": { "id": 42, "is_bot": false }, "chat": { "id": -100, "type": "supergroup" }
        }));
        assert_eq!(
            parse_update(&u, &me()).expect("m").thread_id.as_deref(),
            Some("77")
        );
    }
}
