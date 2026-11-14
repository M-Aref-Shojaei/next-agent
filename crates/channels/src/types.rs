//! Messages and capabilities shared by every channel adapter.

use serde::{Deserialize, Serialize};

/// Direct message or group conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Direct,
    Group,
}

/// One message received from a channel, already parsed by its adapter.
/// Every text field is untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inbound {
    /// Unique per channel account; used as the inbound idempotency key
    /// (Telegram: the `update_id`).
    pub event_id: String,
    pub chat_id: String,
    pub chat_kind: Option<ChatKind>,
    pub sender_id: String,
    /// `@username` or first name, for display.
    pub sender_label: String,
    /// Message id inside the chat (replies thread to it).
    pub message_id: String,
    /// Forum topic / thread, if any.
    pub thread_id: Option<String>,
    pub text: String,
    /// The bot was @-mentioned.
    pub mentioned: bool,
    /// The message replies to one of the bot's messages.
    pub reply_to_bot: bool,
    /// Kind of attached media (`photo`, `document`, `voice` …), if any.
    pub media: Option<String>,
}

impl Inbound {
    #[must_use]
    pub fn kind(&self) -> ChatKind {
        self.chat_kind.unwrap_or(ChatKind::Direct)
    }
}

/// A message to send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outbound {
    pub chat_id: String,
    pub text: String,
    /// Reply to this message id (threads the answer in groups).
    pub reply_to: Option<String>,
    pub thread_id: Option<String>,
}

/// A sent message, so it can be edited later (streaming previews).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRef {
    pub chat_id: String,
    pub message_id: String,
}

/// What an adapter supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Longest text in one message, in UTF-16 code units.
    pub text_chunk_limit: usize,
    /// Messages can be edited (streaming previews).
    pub edit: bool,
    /// A typing indicator exists.
    pub typing: bool,
    /// Threads / forum topics inside groups.
    pub threads: bool,
    /// Media in and out (not yet for Telegram).
    pub media: bool,
}

/// Identity of the bot account, from the adapter's probe ("Test
/// connection").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BotIdentity {
    pub id: String,
    pub username: String,
    pub name: String,
}
