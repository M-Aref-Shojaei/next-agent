// Design adapted from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/translator/{request,response}/* @ 1855aa4
//! Request/response translation. The OpenAI chat format is the pivot: the
//! agent speaks it, OpenAI-compatible providers take it as is, and
//! [`anthropic`] / [`gemini`] convert requests out of it and responses
//! (whole bodies and SSE streams) back into it.

pub mod anthropic;
pub mod gemini;

use crate::llm::LlmError;
use serde_json::{json, Value};

/// Token counts for one request, OpenAI semantics: `prompt` includes
/// `cached` and `cache_creation`; `completion` includes `reasoning`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt: i64,
    pub completion: i64,
    pub cached: i64,
    pub cache_creation: i64,
    pub reasoning: i64,
}

impl Usage {
    /// Read an OpenAI `usage` object (also accepts the extra
    /// `cache_creation_input_tokens` field our decoders add).
    #[must_use]
    pub fn from_openai(usage: &Value) -> Option<Self> {
        let int = |value: Option<&Value>| value.and_then(Value::as_i64).unwrap_or(0);
        if !usage.is_object() {
            return None;
        }
        Some(Self {
            prompt: int(usage.get("prompt_tokens")),
            completion: int(usage.get("completion_tokens")),
            cached: int(usage.pointer("/prompt_tokens_details/cached_tokens")),
            cache_creation: int(usage.get("cache_creation_input_tokens")).max(int(
                usage.pointer("/prompt_tokens_details/cache_write_tokens")
            )),
            reasoning: int(usage.pointer("/completion_tokens_details/reasoning_tokens")),
        })
    }

    /// OpenAI-shaped `usage` object.
    #[must_use]
    pub fn to_openai(&self) -> Value {
        json!({
            "prompt_tokens": self.prompt,
            "completion_tokens": self.completion,
            "total_tokens": self.prompt + self.completion,
            "prompt_tokens_details": {"cached_tokens": self.cached},
            "completion_tokens_details": {"reasoning_tokens": self.reasoning},
            "cache_creation_input_tokens": self.cache_creation,
        })
    }
}

/// Turns one provider SSE event into OpenAI `chat.completion.chunk`s.
pub trait StreamDecoder: Send {
    /// `event` is the SSE `event:` name (may be empty), `data` the parsed payload.
    ///
    /// # Errors
    /// [`LlmError::Protocol`] for provider error events.
    fn decode(&mut self, event: &str, data: &Value) -> Result<Vec<Value>, LlmError>;
}

/// OpenAI-compatible streams need no translation.
#[derive(Debug, Default)]
pub struct OpenAiDecoder;

impl StreamDecoder for OpenAiDecoder {
    fn decode(&mut self, _event: &str, data: &Value) -> Result<Vec<Value>, LlmError> {
        Ok(vec![data.clone()])
    }
}

/// A streamed delta chunk in OpenAI shape.
pub(crate) fn chunk(delta: Value, finish: Option<&str>) -> Value {
    let mut out = json!({
        "object": "chat.completion.chunk",
        "choices": [{"index": 0, "delta": null, "finish_reason": finish}],
    });
    out["choices"][0]["delta"] = delta;
    out
}

/// The trailing usage-only chunk (`stream_options.include_usage` shape).
pub(crate) fn usage_chunk(usage: &Usage) -> Value {
    json!({"object": "chat.completion.chunk", "choices": [], "usage": usage.to_openai()})
}

/// `data:<mime>;base64,<payload>` → `(mime, payload)`.
pub(crate) fn parse_data_uri(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64")?;
    Some((
        if mime.is_empty() {
            "application/octet-stream"
        } else {
            mime
        },
        payload,
    ))
}

/// Best-effort MIME type for a remote file URL.
pub(crate) fn guess_mime(url: &str) -> &'static str {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "image/jpeg",
    }
}

/// Content of an OpenAI message as parts: a plain string becomes one text part.
pub(crate) fn content_parts(message: &Value) -> Vec<Value> {
    match message.get("content") {
        Some(Value::String(text)) => vec![json!({"type": "text", "text": text})],
        Some(Value::Array(parts)) => parts.clone(),
        _ => Vec::new(),
    }
}

/// Plain text of an OpenAI message (text parts joined).
pub(crate) fn content_text(message: &Value) -> String {
    content_parts(message)
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tool-call arguments (a JSON string) as an object; invalid JSON is wrapped.
pub(crate) fn parse_arguments(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value @ Value::Object(_)) => value,
        Ok(other) => json!({ "value": other }),
        Err(_) => json!({ "_raw": raw }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_round_trips_through_openai_shape() {
        let usage = Usage {
            prompt: 10,
            completion: 5,
            cached: 3,
            cache_creation: 2,
            reasoning: 1,
        };
        assert_eq!(Usage::from_openai(&usage.to_openai()), Some(usage));
        assert_eq!(Usage::from_openai(&Value::Null), None);
    }

    #[test]
    fn data_uris_and_mimes() {
        assert_eq!(
            parse_data_uri("data:image/png;base64,AAAA"),
            Some(("image/png", "AAAA"))
        );
        assert_eq!(parse_data_uri("https://x/y.png"), None);
        assert_eq!(guess_mime("https://x/doc.PDF?x=1"), "application/pdf");
        assert_eq!(parse_arguments("{\"a\":1}"), json!({"a":1}));
        assert_eq!(parse_arguments(""), json!({}));
        assert_eq!(parse_arguments("nope"), json!({"_raw":"nope"}));
    }
}
