// Ported from OmniRoute (MIT, © 2026 diegosouzapw): open-sse/services/cache/* @ v3.8.53 (exact layer only)
//! Exact-match response cache.
//!
//! **Key:** SHA-256 of canonical JSON (sorted keys) of the endpoint
//! (connection id + base URL), the model, the token-saver settings, the
//! messages and every other request field except bookkeeping ([`IGNORED`]).
//! Messages keep only `role`, `content`, `tool_calls`, `tool_call_id` and
//! `name`, so bookkeeping fields do not split the cache. Nothing account- or
//! key-related is part of the key, and the key is a one-way hash: two requests
//! share an entry only when the model would see byte-identical input.
//!
//! **When:** the cache is off by default. When on, only requests that set
//! `temperature` to 0 are cached (an unset temperature means the provider's
//! default, usually sampled) unless "any temperature" is enabled,
//! and never requests with `n` > 1 or a per-request opt-out.
//!
//! **What:** the streamed text deltas in order plus the final answer; a hit
//! replays the deltas through the same callback a live stream uses. Any
//! cache error is logged and the request goes to the provider as usual.
//!
//! **Privacy:** entries live in the local metadata database (like task
//! logs) and may contain whatever the model answered, so the cache is opt-in,
//! has a TTL and a size cap, and Settings has a "Clear cache" button. The
//! agent's system prompt names the workspace, so tasks in different
//! workspaces do not share entries.
//!
//! **Semantic layer:** not built. [`SimilarAnswers`] is the seam: a later
//! embedding-based lookup plugs in after an exact miss.

use crate::llm::{Completion, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Bumped when the key or entry format changes.
const SCHEMA: u32 = 3;

/// Body fields that do not change the answer (or are keyed separately).
const IGNORED: &[&str] = &[
    "model",
    "messages",
    "stream",
    "stream_options",
    "user",
    "metadata",
    "prompt_cache_key",
];

/// User settings for the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ResponseCacheSettings {
    pub enabled: bool,
    pub ttl_secs: u32,
    pub max_entries: u32,
    /// Also cache requests with a non-zero temperature.
    pub any_temperature: bool,
}

impl Default for ResponseCacheSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_secs: 3600,
            max_entries: 500,
            any_temperature: false,
        }
    }
}

/// Whether `body` may be served from or stored in the cache.
#[must_use]
pub fn eligible(body: &Value, settings: &ResponseCacheSettings) -> bool {
    // `n` may arrive as 2 or 2.0; anything above one answer is not cached.
    if !settings.enabled
        || body
            .get("n")
            .and_then(Value::as_f64)
            .is_some_and(|n| n > 1.0)
    {
        return false;
    }
    let temperature = body.get("temperature").and_then(Value::as_f64);
    settings.any_temperature || temperature == Some(0.0)
}

fn message(m: &Value) -> Value {
    let mut out = Map::new();
    for field in ["role", "content", "tool_calls", "tool_call_id", "name"] {
        if let Some(v) = m.get(field).filter(|v| !v.is_null()) {
            out.insert(field.into(), v.clone());
        }
    }
    Value::Object(out)
}

/// Whether a provider answer may be stored: only complete answers
/// (`stop` or `tool_calls`), never truncated, filtered or unterminated ones.
#[must_use]
pub fn storable(finish_reason: Option<&str>) -> bool {
    matches!(finish_reason, Some("stop" | "tool_calls"))
}

/// Cache key for `body` sent to `model` on one `endpoint` (the connection
/// id and base URL, so custom endpoints never share entries). `salt` holds
/// request-shaping settings applied after the key is taken (token savers).
#[must_use]
pub fn key(body: &Value, endpoint: &str, model: &str, salt: &Value) -> String {
    let messages: Vec<Value> = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(message)
        .collect();
    // Every request field shapes the answer except these, so the key hashes
    // the whole body minus them (safer than listing the fields to include).
    let mut params = body.as_object().cloned().unwrap_or_default();
    for skip in IGNORED {
        params.remove(*skip);
    }
    let canonical = json!({
        "v": SCHEMA,
        "endpoint": endpoint,
        "model": model,
        "salt": salt,
        "messages": messages,
        "params": params,
    });
    // serde_json maps are sorted by key (no `preserve_order`), so this text is canonical.
    crate::hash::sha256_hex(canonical.to_string().as_bytes())
}

/// A stored answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub deltas: Vec<String>,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    /// Reasoning blocks to re-remember for the tool calls on a hit.
    #[serde(default)]
    pub reasoning_details: Vec<Value>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: f64,
}

impl Entry {
    #[must_use]
    pub fn completion(&self) -> Completion {
        Completion {
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
            finish_reason: self.finish_reason.clone(),
            // Served from cache: the provider measured nothing this time.
            usage: None,
        }
    }
}

/// The seam for a semantic (embedding) layer, consulted after an exact miss.
pub trait SimilarAnswers: Send + Sync {
    /// A stored answer close enough to `body` for this provider and model.
    fn nearest(&self, body: &Value, provider: &str, model: &str) -> Option<Entry>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Value {
        json!({"model": "fast", "temperature": 0, "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "hi"}
        ]})
    }

    fn k(body: &Value, endpoint: &str, model: &str) -> String {
        key(body, endpoint, model, &json!({"saver": false}))
    }

    #[test]
    fn keys_are_stable_and_sensitive_to_what_the_model_sees() {
        let base = k(&body(), "c1|https://api.openai.com/v1", "gpt-4o");
        assert_eq!(base.len(), 64);
        assert_eq!(base, k(&body(), "c1|https://api.openai.com/v1", "gpt-4o"));
        let mut extra = body();
        extra["messages"][1]["reasoning_details"] = json!([]);
        extra["stream"] = json!(true);
        extra["user"] = json!("someone");
        assert_eq!(
            k(&extra, "c1|https://api.openai.com/v1", "gpt-4o"),
            base,
            "bookkeeping is ignored"
        );
        let mut changed = body();
        changed["messages"][1]["content"] = json!("hello");
        assert_ne!(k(&changed, "c1|https://api.openai.com/v1", "gpt-4o"), base);
        assert_ne!(
            k(&body(), "c1|https://api.openai.com/v1", "gpt-4o-mini"),
            base
        );
        let mut warm = body();
        warm["temperature"] = json!(0.7);
        assert_ne!(k(&warm, "c1|https://api.openai.com/v1", "gpt-4o"), base);
    }

    /// H1/H2/L5 regressions: output limits, stops, seeds, endpoints and saver
    /// settings all split the cache.
    #[test]
    fn keys_split_by_output_contract_endpoint_and_savers() {
        let base = k(&body(), "c1|https://a/v1", "m");
        for (field, value) in [
            ("max_tokens", json!(10)),
            ("max_completion_tokens", json!(10)),
            ("stop", json!(["\n"])),
            ("seed", json!(7)),
            ("reasoning_effort", json!("high")),
            ("frequency_penalty", json!(0.5)),
            ("top_k", json!(3)),
            ("parallel_tool_calls", json!(false)),
        ] {
            let mut changed = body();
            changed[field] = value;
            assert_ne!(k(&changed, "c1|https://a/v1", "m"), base, "{field}");
        }
        assert_ne!(
            k(&body(), "c2|https://a/v1", "m"),
            base,
            "another connection"
        );
        assert_ne!(k(&body(), "c1|https://b/v1", "m"), base, "another base URL");
        assert_ne!(
            key(&body(), "c1|https://a/v1", "m", &json!({"saver": true})),
            base,
            "savers"
        );
    }

    #[test]
    fn only_complete_answers_are_storable() {
        assert!(storable(Some("stop")) && storable(Some("tool_calls")));
        for reason in [None, Some("length"), Some("content_filter"), Some("error")] {
            assert!(!storable(reason), "{reason:?}");
        }
    }

    #[test]
    fn eligibility_needs_the_switch_and_a_zero_temperature() {
        let on = ResponseCacheSettings {
            enabled: true,
            ..ResponseCacheSettings::default()
        };
        assert!(!eligible(&body(), &ResponseCacheSettings::default()));
        assert!(eligible(&body(), &on));
        assert!(
            !eligible(&json!({"messages": []}), &on),
            "unset = provider default"
        );
        let warm = json!({"temperature": 0.5});
        assert!(!eligible(&warm, &on));
        assert!(eligible(
            &warm,
            &ResponseCacheSettings {
                any_temperature: true,
                ..on
            }
        ));
        assert!(!eligible(&json!({"n": 2}), &on));
        assert!(
            !eligible(&json!({"temperature": 0, "n": 2.0}), &on),
            "n as a float"
        );
        assert!(eligible(&json!({"temperature": 0, "n": 1}), &on));
    }
}
