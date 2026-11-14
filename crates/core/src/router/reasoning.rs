// Design adapted from OpenRouter docs: https://openrouter.ai/docs/use-cases/reasoning-tokens
//! One `reasoning` setting per model role, mapped to each provider.
//!
//! The gateway puts `{"effort": …}` or `{"max_tokens": …}` in the OpenAI body
//! as `reasoning` (OpenRouter's shape) unless the caller set one, then:
//! - **OpenRouter** gets it unchanged;
//! - **other OpenAI-compatible** providers get `reasoning_effort`
//!   (`max` → `high`; a token budget becomes the effort whose share of the
//!   output limit it is closest to);
//! - **Anthropic** gets `thinking.budget_tokens` =
//!   `max(min(max_output·share, 128000), 1024)` and `max_tokens` raised to the
//!   output ceiling so it stays above the budget; `temperature`/`top_p` are
//!   dropped (Anthropic rejects them with thinking);
//! - **Gemini 3** gets `thinkingLevel` (`max` → `high`), **Gemini 2.5**
//!   `thinkingBudget`.
//!
//! Effort shares of the output limit: max 95%, high 80%, medium 50%, low 20%.
//!
//! Reasoning blocks that must go back to the provider during tool use
//! (Anthropic thinking signatures, Gemini thought signatures) travel as
//! OpenRouter-style `reasoning_details` on the assistant message; the gateway
//! remembers them per tool call (see `GatewayState::remember_reasoning`) and
//! puts them back unchanged when the agent sends the conversation again.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Reasoning effort; `Off` sends nothing (the provider's default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    #[default]
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl Effort {
    #[must_use]
    pub fn share(self) -> f64 {
        match self {
            Self::Off => 0.0,
            Self::Low => 0.2,
            Self::Medium => 0.5,
            Self::High => 0.8,
            Self::Max => 0.95,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "minimal" | "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" | "max" => Self::Max,
            "off" | "none" => Self::Off,
            _ => return None,
        })
    }

    /// `max` for providers whose top level is `high`.
    fn capped_high(self) -> Self {
        if self == Self::Max {
            Self::High
        } else {
            self
        }
    }

    /// The effort whose share is closest to `share`.
    fn nearest(share: f64) -> Self {
        [Self::Low, Self::Medium, Self::High, Self::Max]
            .into_iter()
            .min_by(|a, b| {
                (a.share() - share)
                    .abs()
                    .total_cmp(&(b.share() - share).abs())
            })
            .unwrap_or(Self::Medium)
    }
}

/// The per-role setting: an effort, or an explicit token budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReasoningSetting {
    pub effort: Effort,
    /// Wins over `effort` when set.
    pub max_tokens: Option<u32>,
}

impl ReasoningSetting {
    /// The OpenAI-body `reasoning` object, or `None` when off.
    #[must_use]
    pub fn to_body(self) -> Option<Value> {
        match (self.max_tokens, self.effort) {
            (Some(n), _) if n > 0 => Some(json!({"max_tokens": n})),
            (_, Effort::Off) => None,
            (_, effort) => Some(json!({"effort": effort.as_str()})),
        }
    }
}

/// What a body's `reasoning` asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ask {
    Effort(Effort),
    Budget(u64),
}

fn ask(body: &Value) -> Option<Ask> {
    let reasoning = body.get("reasoning")?;
    if reasoning.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    if let Some(n) = reasoning.get("max_tokens").and_then(Value::as_u64) {
        return (n > 0).then_some(Ask::Budget(n));
    }
    let effort = reasoning
        .get("effort")
        .and_then(Value::as_str)
        .and_then(Effort::parse)
        .unwrap_or(Effort::Medium);
    (effort != Effort::Off).then_some(Ask::Effort(effort))
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn budget(ask: Ask, max_output: u32) -> u64 {
    match ask {
        Ask::Budget(n) => n,
        Ask::Effort(e) => (f64::from(max_output) * e.share()).round() as u64,
    }
}

#[allow(clippy::cast_precision_loss)]
fn effort(ask: Ask, max_output: u32) -> Effort {
    match ask {
        Ask::Effort(e) => e,
        Ask::Budget(n) => Effort::nearest(n as f64 / f64::from(max_output.max(1))),
    }
}

fn strip_details(body: &mut Value) {
    for message in body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(map) = message.as_object_mut() {
            map.remove("reasoning_details");
        }
    }
}

/// OpenAI-compatible body. OpenRouter keeps `reasoning` and the details.
pub fn apply_openai(body: &mut Value, provider: &str, max_output: u32) {
    if provider == "openrouter" {
        return;
    }
    let wanted = ask(body);
    if let Some(map) = body.as_object_mut() {
        map.remove("reasoning");
    }
    strip_details(body);
    if let Some(wanted) = wanted {
        let level = effort(wanted, max_output).capped_high();
        body["reasoning_effort"] = json!(level.as_str());
    }
}

/// Translated Anthropic body; `openai` is the source body.
pub fn apply_anthropic(body: &mut Value, openai: &Value, ceiling: u32) {
    let Some(wanted) = ask(openai) else { return };
    let tokens = budget(wanted, ceiling).clamp(1024, 128_000);
    if u64::from(ceiling) <= tokens {
        return; // No room for an answer after thinking.
    }
    body["thinking"] = json!({"type": "enabled", "budget_tokens": tokens});
    body["max_tokens"] = json!(ceiling);
    if let Some(map) = body.as_object_mut() {
        map.remove("temperature");
        map.remove("top_p");
    }
}

/// Translated Gemini body.
pub fn apply_gemini(body: &mut Value, openai: &Value, model: &str, max_output: u32) {
    let Some(wanted) = ask(openai) else { return };
    let config = if model.starts_with("gemini-3") {
        let level = effort(wanted, max_output).capped_high();
        json!({"thinkingLevel": level.as_str()})
    } else if model.starts_with("gemini-2.5") {
        json!({"thinkingBudget": budget(wanted, max_output).min(32_768)})
    } else {
        return;
    };
    let Some(map) = body.as_object_mut() else {
        return;
    };
    let generation = map
        .entry("generationConfig")
        .or_insert_with(|| Value::Object(Map::new()));
    generation["thinkingConfig"] = config;
}

/// Merge streamed `reasoning_details` deltas: entries with the same `index`
/// (and type) concatenate their `text`/`summary`/`data` and keep the last
/// `signature`; others are appended in order.
pub fn merge_details(into: &mut Vec<Value>, deltas: &[Value]) {
    for delta in deltas {
        let index = delta.get("index").and_then(Value::as_u64);
        let kind = delta.get("type").cloned();
        let existing = index.and_then(|i| {
            into.iter_mut().find(|d| {
                d.get("index").and_then(Value::as_u64) == Some(i) && d.get("type").cloned() == kind
            })
        });
        let Some(existing) = existing else {
            into.push(delta.clone());
            continue;
        };
        for field in ["text", "summary", "data"] {
            let Some(more) = delta.get(field).and_then(Value::as_str) else {
                continue;
            };
            match existing.get_mut(field) {
                Some(Value::String(text)) => text.push_str(more),
                _ => existing[field] = json!(more),
            }
        }
        if let Some(signature) = delta.get("signature").filter(|s| !s.is_null()) {
            existing["signature"] = signature.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::needless_pass_by_value)] // Reads better at call sites.
    fn with(reasoning: Value) -> Value {
        json!({"model": "m", "temperature": 0.2, "top_p": 0.9, "reasoning": reasoning, "messages": [
            {"role": "assistant", "content": "x", "reasoning_details": [{"type": "reasoning.text", "text": "t"}]}
        ]})
    }

    #[test]
    fn settings_become_a_body_field() {
        assert_eq!(ReasoningSetting::default().to_body(), None);
        let high = ReasoningSetting {
            effort: Effort::High,
            max_tokens: None,
        };
        assert_eq!(high.to_body(), Some(json!({"effort": "high"})));
        let budget = ReasoningSetting {
            effort: Effort::Low,
            max_tokens: Some(5000),
        };
        assert_eq!(budget.to_body(), Some(json!({"max_tokens": 5000})));
    }

    #[test]
    fn openai_gets_reasoning_effort_and_openrouter_the_original() {
        let mut body = with(json!({"effort": "max"}));
        apply_openai(&mut body, "openai", 32_000);
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("reasoning").is_none());
        assert!(body["messages"][0].get("reasoning_details").is_none());
        let mut budget = with(json!({"max_tokens": 16_000}));
        apply_openai(&mut budget, "deepseek", 32_000);
        assert_eq!(
            budget["reasoning_effort"], "medium",
            "16k of 32k is closest to 50%"
        );
        let mut routed = with(json!({"effort": "max"}));
        apply_openai(&mut routed, "openrouter", 32_000);
        assert_eq!(
            routed,
            with(json!({"effort": "max"})),
            "passed through unchanged"
        );
        let mut off = with(json!({"effort": "high", "enabled": false}));
        apply_openai(&mut off, "openai", 32_000);
        assert!(off.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_gets_a_bounded_thinking_budget() {
        let source = with(json!({"effort": "high"}));
        let mut body = json!({"max_tokens": 4096, "temperature": 0.2, "top_p": 0.9});
        apply_anthropic(&mut body, &source, 64_000);
        assert_eq!(
            body["thinking"],
            json!({"type": "enabled", "budget_tokens": 51_200})
        );
        assert_eq!(body["max_tokens"], 64_000, "above the budget");
        assert!(body.get("temperature").is_none() && body.get("top_p").is_none());
        let mut low = json!({});
        apply_anthropic(&mut low, &with(json!({"effort": "low"})), 2000);
        assert_eq!(low["thinking"]["budget_tokens"], 1024, "at least 1024");
        let mut huge = json!({});
        apply_anthropic(&mut huge, &with(json!({"effort": "max"})), 200_000);
        assert_eq!(huge["thinking"]["budget_tokens"], 128_000, "at most 128k");
        let mut tight = json!({"max_tokens": 1000});
        apply_anthropic(&mut tight, &with(json!({"effort": "low"})), 1000);
        assert!(
            tight.get("thinking").is_none(),
            "no room left for an answer"
        );
    }

    #[test]
    fn gemini_gets_a_level_or_a_budget_by_generation() {
        let source = with(json!({"effort": "max"}));
        let mut three = json!({"generationConfig": {"maxOutputTokens": 100}});
        apply_gemini(&mut three, &source, "gemini-3-pro-preview", 65_536);
        assert_eq!(
            three["generationConfig"]["thinkingConfig"],
            json!({"thinkingLevel": "high"})
        );
        assert_eq!(three["generationConfig"]["maxOutputTokens"], 100);
        let mut two = json!({});
        apply_gemini(
            &mut two,
            &with(json!({"effort": "medium"})),
            "gemini-2.5-flash",
            65_536,
        );
        assert_eq!(
            two["generationConfig"]["thinkingConfig"],
            json!({"thinkingBudget": 32_768})
        );
        let mut budget = json!({});
        apply_gemini(
            &mut budget,
            &with(json!({"max_tokens": 2048})),
            "gemini-2.5-pro",
            65_536,
        );
        assert_eq!(
            budget["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            2048
        );
        let mut old = json!({});
        apply_gemini(&mut old, &source, "gemini-2.0-flash", 8192);
        assert_eq!(old, json!({}));
    }

    #[test]
    fn streamed_details_merge_by_index() {
        let mut all = Vec::new();
        merge_details(
            &mut all,
            &[json!({"type": "reasoning.text", "index": 0, "text": "Let me "})],
        );
        merge_details(
            &mut all,
            &[json!({"type": "reasoning.text", "index": 0, "text": "think"})],
        );
        merge_details(
            &mut all,
            &[json!({"type": "reasoning.text", "index": 0, "signature": "sig"})],
        );
        merge_details(
            &mut all,
            &[json!({"type": "reasoning.encrypted", "index": 1, "data": "xyz"})],
        );
        assert_eq!(
            all,
            [
                json!({"type": "reasoning.text", "index": 0, "text": "Let me think", "signature": "sig"}),
                json!({"type": "reasoning.encrypted", "index": 1, "data": "xyz"}),
            ]
        );
    }
}
