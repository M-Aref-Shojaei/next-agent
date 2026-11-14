// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/channels/plugins/types.adapters.ts (ChannelGatewayAdapter, ChannelOutboundAdapter, ChannelPairingAdapter, ChannelThreadingAdapter) @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! The adapter contract every channel implements.
//!
//! OpenClaw splits this across several plugin adapters (gateway, outbound,
//! pairing, threading, capabilities). We keep one object-safe trait; the
//! supervisor ([`crate::supervisor`]) owns reconnects and health, the hub
//! ([`crate::hub`]) owns policy, routing and task runs. Adapters only move
//! bytes.

use crate::config::ChannelConfig;
use crate::types::{BotIdentity, Capabilities, Inbound, MessageRef, Outbound};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

/// A boxed, sendable future (keeps the trait object-safe without
/// `async-trait`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a channel call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// Worth retrying (network, rate limit, server error). `retry_after`
    /// is the wait the service asked for, in seconds.
    Transient {
        message: String,
        retry_after: Option<u64>,
    },
    /// Retrying will not help (bad token, bot removed); needs the owner.
    Fatal(String),
}

impl ChannelError {
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self::Transient {
            message: message.into(),
            retry_after: None,
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Transient { message, .. } | Self::Fatal(message) => message,
        }
    }
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ChannelError {}

/// Hook an adapter calls once it is receiving (first successful poll), so
/// the supervisor can report `connected`.
pub type ConnectedHook = Box<dyn Fn() + Send + Sync>;

/// A chat channel account (one Telegram bot …).
pub trait Channel: Send + Sync {
    /// The configuration this adapter was built from (`config`).
    fn config(&self) -> &ChannelConfig;

    /// Feature flags and limits (`capabilities`).
    fn capabilities(&self) -> Capabilities;

    /// Check the credentials and return the bot identity ("Test
    /// connection").
    fn probe(&self) -> BoxFuture<'_, Result<BotIdentity, ChannelError>>;

    /// Receive until an error: push every inbound message into `inbound`
    /// and call `connected` once receiving works. Returns `Ok(())` only
    /// when `inbound` is closed (shutdown).
    fn start(
        &self,
        inbound: mpsc::Sender<Inbound>,
        connected: ConnectedHook,
    ) -> BoxFuture<'_, Result<(), ChannelError>>;

    /// Send one message, already within the chunk limit (`send`).
    fn send(&self, message: Outbound) -> BoxFuture<'_, Result<MessageRef, ChannelError>>;

    /// Replace a sent message's text (streaming previews). Only called when
    /// [`Capabilities::edit`] is set.
    fn edit(&self, target: MessageRef, text: String) -> BoxFuture<'_, Result<(), ChannelError>>;

    /// Show "typing…" in a chat for a few seconds. Best effort.
    fn typing(&self, chat_id: String) -> BoxFuture<'_, Result<(), ChannelError>>;

    /// Where replies to `message` go (`threading`): the same chat and
    /// thread, as a reply in groups.
    fn reply_target(&self, message: &Inbound, text: String) -> Outbound {
        Outbound {
            chat_id: message.chat_id.clone(),
            text,
            reply_to: (message.kind() == crate::types::ChatKind::Group)
                .then(|| message.message_id.clone()),
            thread_id: message.thread_id.clone(),
        }
    }

    /// How a sender is shown on a pairing request (`pairing`).
    fn pairing_label(&self, message: &Inbound) -> String {
        if message.sender_label.is_empty() {
            message.sender_id.clone()
        } else {
            message.sender_label.clone()
        }
    }
}
