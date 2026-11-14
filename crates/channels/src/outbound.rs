// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/channels/draft-stream-loop.ts (throttled preview edits), src/channels/plugins/outbound.types.ts (idempotency keys) @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! Outbound pipeline: chunk to the channel's limit, send each chunk once
//! (idempotency keys survive restarts), and stream previews by editing one
//! message while the agent is still writing.

use crate::channel::{Channel, ChannelError};
use crate::chunk::chunk_text;
use crate::state::ChannelState;
use crate::types::{Inbound, MessageRef};
use std::time::{Duration, Instant};

/// Shortest gap between two preview edits (Telegram rate-limits edits).
pub const PREVIEW_THROTTLE: Duration = Duration::from_millis(1_500);

/// Idempotency key of chunk `part` of the reply to `message`.
#[must_use]
pub fn reply_key(channel_id: &str, message: &Inbound, part: usize) -> String {
    format!(
        "out:{channel_id}:{}:{}:{part}",
        message.chat_id, message.message_id
    )
}

/// Idempotency key of an inbound event.
#[must_use]
pub fn inbound_key(channel_id: &str, event_id: &str) -> String {
    format!("in:{channel_id}:{event_id}")
}

/// Send `text` as a reply to `message`, chunked. A chunk whose key was
/// already recorded is skipped, so a redelivered message is not answered
/// twice. Returns how many chunks were actually sent.
///
/// # Errors
/// The first send failure (later chunks are not sent; a retry resumes from
/// the failed one).
pub async fn send_reply(
    channel: &dyn Channel,
    state: &dyn ChannelState,
    message: &Inbound,
    text: &str,
) -> Result<usize, ChannelError> {
    send_chunks(channel, state, message, text, 0).await
}

async fn send_chunks(
    channel: &dyn Channel,
    state: &dyn ChannelState,
    message: &Inbound,
    text: &str,
    first_part: usize,
) -> Result<usize, ChannelError> {
    let limit = channel.capabilities().text_chunk_limit;
    let channel_id = channel.config().id.clone();
    let mut sent = 0;
    for (index, chunk) in chunk_text(text, limit).into_iter().enumerate() {
        let key = reply_key(&channel_id, message, first_part + index);
        if state.claimed(&key).ok().flatten().is_some() {
            continue;
        }
        let reference = channel.send(channel.reply_target(message, chunk)).await?;
        let _ = state.claim(&key, &reference.message_id);
        sent += 1;
    }
    Ok(sent)
}

/// A reply that is shown while it is being written, by editing one
/// message, then finished in place (plus extra chunks if it grew past the
/// limit).
pub struct Preview<'a> {
    channel: &'a dyn Channel,
    state: &'a dyn ChannelState,
    message: &'a Inbound,
    text: String,
    sent: Option<MessageRef>,
    shown: String,
    last_edit: Option<Instant>,
    throttle: Duration,
}

impl<'a> Preview<'a> {
    #[must_use]
    pub fn new(channel: &'a dyn Channel, state: &'a dyn ChannelState, message: &'a Inbound) -> Self {
        Self {
            channel,
            state,
            message,
            text: String::new(),
            sent: None,
            shown: String::new(),
            last_edit: None,
            throttle: PREVIEW_THROTTLE,
        }
    }

    #[must_use]
    pub fn with_throttle(mut self, throttle: Duration) -> Self {
        self.throttle = throttle;
        self
    }

    /// Whether previews are possible on this channel.
    fn enabled(&self) -> bool {
        self.channel.capabilities().edit
    }

    /// Append streamed text and refresh the preview if the throttle allows.
    pub async fn push(&mut self, delta: &str) {
        self.text.push_str(delta);
        if !self.enabled() || self.text.trim().is_empty() {
            return;
        }
        let due = self.last_edit.is_none_or(|t| t.elapsed() >= self.throttle);
        if due {
            self.flush().await;
        }
    }

    async fn flush(&mut self) {
        let limit = self.channel.capabilities().text_chunk_limit;
        // Show the first chunk's worth while streaming, with an ellipsis.
        let mut view = chunk_text(&self.text, limit.saturating_sub(2))
            .into_iter()
            .next()
            .unwrap_or_default();
        view.push_str(" …");
        if view == self.shown {
            return;
        }
        let ok = match &self.sent {
            Some(target) => self.channel.edit(target.clone(), view.clone()).await.is_ok(),
            None => match self
                .channel
                .send(self.channel.reply_target(self.message, view.clone()))
                .await
            {
                Ok(reference) => {
                    self.sent = Some(reference);
                    true
                }
                Err(_) => false,
            },
        };
        if ok {
            self.shown = view;
        }
        self.last_edit = Some(Instant::now());
    }

    /// Replace the preview with the final `text` (or send it normally if no
    /// preview was shown). Chunk 0 is the preview message itself.
    ///
    /// # Errors
    /// A send failure for the final text.
    pub async fn finish(self, text: &str) -> Result<usize, ChannelError> {
        let Some(target) = self.sent.clone() else {
            return send_reply(self.channel, self.state, self.message, text).await;
        };
        let channel_id = self.channel.config().id.clone();
        let limit = self.channel.capabilities().text_chunk_limit;
        let chunks = chunk_text(text, limit);
        let first = chunks.first().cloned().unwrap_or_else(|| "(no answer)".into());
        let first_key = reply_key(&channel_id, self.message, 0);
        if self.state.claimed(&first_key).ok().flatten().is_none() {
            self.channel.edit(target.clone(), first).await?;
            let _ = self.state.claim(&first_key, &target.message_id);
        }
        let rest = chunks.get(1..).unwrap_or_default().join("\n");
        let more = if rest.is_empty() {
            0
        } else {
            send_chunks(self.channel, self.state, self.message, &rest, 1).await?
        };
        Ok(1 + more)
    }
}
