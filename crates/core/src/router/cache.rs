// Ported from OmniRoute (MIT, © 2026 diegosouzapw): open-sse/services/cache/* @ v3.8.53 (exact layer only)
//! Exact-match response cache.
//!
//! **Key:** SHA-256 of canonical JSON (sorted keys) of `{v, provider, model,
//! messages, temperature, top_p, tools, tool_choice, response_format}`.
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
const SCHEMA: u32 = 1;

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
    if !settings.enabled || body.get("n").and_then(Value::as_u64).is_some_and(|n| n > 1) {
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

/// Cache key for `body` sent to `provider`/`model`.
#[must_use]
pub fn key(body: &Value, provider: &str, model: &str) -> String {
    let messages: Vec<Value> = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(message)
        .collect();
    let canonical = json!({
        "v": SCHEMA,
        "provider": provider,
        "model": model,
        "messages": messages,
        "temperature": body.get("temperature").and_then(Value::as_f64).unwrap_or(0.0),
        "top_p": body.get("top_p").and_then(Value::as_f64).unwrap_or(1.0),
        "tools": body.get("tools"),
        "tool_choice": body.get("tool_choice"),
        "response_format": body.get("response_format"),
        "reasoning": body.get("reasoning"),
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

    #[test]
    fn keys_are_stable_and_sensitive_to_what_the_model_sees() {
        let base = key(&body(), "openai", "gpt-4o");
        assert_eq!(base.len(), 64);
        assert_eq!(base, key(&body(), "openai", "gpt-4o"));
        let mut extra = body();
        extra["messages"][1]["reasoning_details"] = json!([]);
        extra["stream"] = json!(true);
        assert_eq!(
            key(&extra, "openai", "gpt-4o"),
            base,
            "bookkeeping is ignored"
        );
        let mut changed = body();
        changed["messages"][1]["content"] = json!("hello");
        assert_ne!(key(&changed, "openai", "gpt-4o"), base);
        assert_ne!(key(&body(), "openai", "gpt-4o-mini"), base);
        assert_ne!(key(&body(), "openrouter", "gpt-4o"), base);
        let mut warm = body();
        warm["temperature"] = json!(0.7);
        assert_ne!(key(&warm, "openai", "gpt-4o"), base);
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
    }
}
