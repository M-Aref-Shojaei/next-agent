//! Minimal Telegram Bot API client over HTTPS (`reqwest`). The only file in
//! the crate that does HTTP (see `scripts/check-boundaries.mjs`).
//!
//! The token is part of every URL (`/bot<token>/<method>`), so it is kept
//! out of `Debug` output and stripped from every error message.

use crate::channel::ChannelError;
use serde_json::{json, Value};
use std::time::Duration;

/// Production Bot API.
pub const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

/// Check a token's shape before it goes into a URL path:
/// `<digits>:<letters, digits, _ or ->`.
#[must_use]
pub fn valid_token(token: &str) -> bool {
    let Some((id, secret)) = token.split_once(':') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && secret.len() >= 20
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A Bot API client for one token.
#[derive(Clone)]
pub struct TelegramApi {
    http: reqwest::Client,
    base: String,
    token: String,
    /// `getUpdates` long-poll timeout.
    poll: Duration,
}

impl std::fmt::Debug for TelegramApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramApi")
            .field("base", &self.base)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl TelegramApi {
    /// # Errors
    /// The token is malformed or the HTTP client cannot be built.
    pub fn new(base: &str, token: &str, poll: Duration) -> Result<Self, ChannelError> {
        let token = token.trim();
        if !valid_token(token) {
            return Err(ChannelError::Fatal(
                "that does not look like a Telegram bot token (expected `123456:ABC…` from @BotFather)".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(poll + Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                ChannelError::Fatal(format!(
                    "could not build the HTTP client: {}",
                    e.without_url()
                ))
            })?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            poll,
        })
    }

    #[must_use]
    pub fn poll_timeout(&self) -> Duration {
        self.poll
    }

    /// Call `method` with a JSON body; returns `result`.
    async fn call(&self, method: &str, body: &Value) -> Result<Value, ChannelError> {
        let url = format!("{}/bot{}/{method}", self.base, self.token);
        let response = self.http.post(url).json(body).send().await.map_err(|e| {
            ChannelError::transient(format!(
                "Telegram unreachable: {}",
                self.scrub(&e.without_url().to_string())
            ))
        })?;
        let status = response.status().as_u16();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        if payload["ok"].as_bool() == Some(true) {
            return Ok(payload["result"].clone());
        }
        let code = payload["error_code"].as_u64().unwrap_or(u64::from(status));
        let description = self.scrub(payload["description"].as_str().unwrap_or("no description"));
        Err(classify(
            code,
            &description,
            payload["parameters"]["retry_after"].as_u64(),
        ))
    }

    /// Belt and braces: never let the token reach a message.
    fn scrub(&self, text: &str) -> String {
        text.replace(&self.token, "<token>")
    }

    /// `getMe`.
    ///
    /// # Errors
    /// See [`classify`].
    pub async fn get_me(&self) -> Result<Value, ChannelError> {
        self.call("getMe", &json!({})).await
    }

    /// `getUpdates` (long poll) for messages after `offset`.
    ///
    /// # Errors
    /// See [`classify`].
    pub async fn get_updates(&self, offset: i64) -> Result<Vec<Value>, ChannelError> {
        let result = self
            .call(
                "getUpdates",
                &json!({
                    "offset": offset,
                    "timeout": self.poll.as_secs(),
                    "allowed_updates": ["message"],
                }),
            )
            .await?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    /// `sendMessage` as plain text; returns the new message id.
    ///
    /// # Errors
    /// See [`classify`].
    pub async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&str>,
        thread_id: Option<&str>,
    ) -> Result<String, ChannelError> {
        let mut body = json!({
            "chat_id": chat_value(chat_id),
            "text": text,
            "link_preview_options": { "is_disabled": true },
        });
        if let Some(id) = reply_to.and_then(|id| id.parse::<i64>().ok()) {
            body["reply_parameters"] =
                json!({ "message_id": id, "allow_sending_without_reply": true });
        }
        if let Some(id) = thread_id.and_then(|id| id.parse::<i64>().ok()) {
            body["message_thread_id"] = json!(id);
        }
        let result = self.call("sendMessage", &body).await?;
        Ok(result["message_id"]
            .as_i64()
            .map_or_else(String::new, |id| id.to_string()))
    }

    /// `editMessageText`. "message is not modified" counts as success.
    ///
    /// # Errors
    /// See [`classify`].
    pub async fn edit_message_text(
        &self,
        chat_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), ChannelError> {
        let body = json!({
            "chat_id": chat_value(chat_id),
            "message_id": message_id.parse::<i64>().unwrap_or_default(),
            "text": text,
            "link_preview_options": { "is_disabled": true },
        });
        match self.call("editMessageText", &body).await {
            Err(error) if error.message().contains("message is not modified") => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// `sendChatAction` `typing`.
    ///
    /// # Errors
    /// See [`classify`].
    pub async fn send_typing(&self, chat_id: &str) -> Result<(), ChannelError> {
        self.call(
            "sendChatAction",
            &json!({ "chat_id": chat_value(chat_id), "action": "typing" }),
        )
        .await
        .map(|_| ())
    }
}

/// Numeric chat ids go out as numbers, `@channel` names as strings.
fn chat_value(chat_id: &str) -> Value {
    chat_id
        .parse::<i64>()
        .map_or_else(|_| json!(chat_id), |id| json!(id))
}

/// Map a Bot API error to transient or fatal.
#[must_use]
pub fn classify(code: u64, description: &str, retry_after: Option<u64>) -> ChannelError {
    match code {
        401 | 404 => ChannelError::Fatal(format!(
            "Telegram rejected the bot token ({code} {description}). Paste a new token from @BotFather in Settings → Channels."
        )),
        409 => ChannelError::transient(format!(
            "Telegram reports a conflict ({description}): another program is polling this bot, or a webhook is set. Stop the other program or remove the webhook."
        )),
        429 => ChannelError::Transient {
            message: format!("Telegram rate limit ({description})"),
            retry_after: retry_after.or(Some(5)),
        },
        _ => ChannelError::transient(format!("Telegram error {code}: {description}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shape() {
        assert!(valid_token("123456:ABCdefGHIjklMNOpqrSTUvwxYZ_-12345"));
        for bad in [
            "",
            "abc",
            "123:",
            ":abcdefghijklmnopqrstuvwxyz",
            "12a:abcdefghijklmnopqrstu",
            "123:abc/../../x-yyyyyyyyyyyyyy",
            "123:short",
        ] {
            assert!(!valid_token(bad), "{bad}");
        }
    }

    #[test]
    fn debug_hides_the_token() {
        let api = TelegramApi::new(
            DEFAULT_BASE_URL,
            "123456:ABCdefGHIjklMNOpqrSTUvwxYZ",
            Duration::from_secs(1),
        )
        .expect("api");
        assert!(!format!("{api:?}").contains("ABCdef"));
    }

    #[test]
    fn errors_are_classified() {
        assert!(matches!(
            classify(401, "Unauthorized", None),
            ChannelError::Fatal(_)
        ));
        assert!(matches!(
            classify(404, "Not Found", None),
            ChannelError::Fatal(_)
        ));
        assert!(matches!(
            classify(409, "Conflict", None),
            ChannelError::Transient { .. }
        ));
        assert_eq!(
            classify(429, "Too Many Requests", Some(7)),
            ChannelError::Transient {
                message: "Telegram rate limit (Too Many Requests)".into(),
                retry_after: Some(7)
            }
        );
        assert!(matches!(
            classify(502, "Bad Gateway", None),
            ChannelError::Transient { .. }
        ));
    }

    #[test]
    fn chat_ids_are_numbers_when_numeric() {
        assert_eq!(chat_value("-1001"), json!(-1001));
        assert_eq!(chat_value("@chan"), json!("@chan"));
    }
}
