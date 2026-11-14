// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/services/combo.js, open-sse/services/accountFallback.js @ 1855aa4
// Ported from OmniRoute (MIT, © 2026 diegosouzapw): open-sse/services/combo/targetSorters.ts @ v3.8.53 (weighted, cost-optimized, lkgp, context-optimized)
//! Combos (named model lists with a strategy), capability reordering and
//! per-account cooldown with exponential backoff.
//!
//! Strategies:
//! - `fallback`: in order;
//! - `round-robin`: rotate the start, advancing after `sticky_limit` uses;
//! - `smart`: a sticky model per session (see [`super::smart`]);
//! - `weighted`: random order drawn by each model's weight (default 1);
//! - `cost-optimized`: cheapest input price first (unknown prices last);
//! - `lkgp`: the last model that answered for this combo first;
//! - `context-optimized`: largest context window first (unknown last).
//!
//! Not ported: 9Router's `fusion`; OmniRoute's `p2c`, `least-used` and
//! `strict-random` (load balancing across many keys).

use super::breaker::Breakers;
use super::smart::{Sticky, DEFAULT_THRESHOLD};
use crate::store::ComboRow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How a combo orders its models.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    #[default]
    Fallback,
    RoundRobin,
    Smart,
    Weighted,
    CostOptimized,
    Lkgp,
    ContextOptimized,
}

impl Strategy {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "fallback" => Some(Self::Fallback),
            "round-robin" => Some(Self::RoundRobin),
            "smart" => Some(Self::Smart),
            "weighted" => Some(Self::Weighted),
            "cost-optimized" => Some(Self::CostOptimized),
            "lkgp" => Some(Self::Lkgp),
            "context-optimized" => Some(Self::ContextOptimized),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::RoundRobin => "round-robin",
            Self::Smart => "smart",
            Self::Weighted => "weighted",
            Self::CostOptimized => "cost-optimized",
            Self::Lkgp => "lkgp",
            Self::ContextOptimized => "context-optimized",
        }
    }
}

/// A named list of `provider/model` targets.
#[derive(Debug, Clone, PartialEq)]
pub struct Combo {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    /// Per-combo strategy; `None` = the global setting.
    pub strategy: Option<Strategy>,
    /// Round-robin: uses before advancing (≥ 1).
    pub sticky_limit: u32,
    /// Smart: switch when confidence drops below this (0..=1).
    pub confidence_threshold: f64,
    /// Weighted: one weight per model (missing = 1).
    pub weights: Vec<u32>,
}

impl Combo {
    /// # Errors
    /// Returns a message if `models` is not a JSON string array.
    pub fn from_row(row: &ComboRow) -> Result<Self, String> {
        let models: Vec<String> = serde_json::from_str(&row.models)
            .map_err(|e| format!("combo {}: bad models: {e}", row.name))?;
        Ok(Self {
            id: row.id.clone(),
            name: row.name.clone(),
            models,
            strategy: row.strategy.as_deref().and_then(Strategy::parse),
            sticky_limit: row
                .sticky_limit
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(1)
                .max(1),
            confidence_threshold: row
                .confidence_threshold
                .filter(|t| (0.0..=1.0).contains(t))
                .unwrap_or(DEFAULT_THRESHOLD),
            weights: row
                .weights
                .as_deref()
                .and_then(|w| serde_json::from_str(w).ok())
                .unwrap_or_default(),
        })
    }
}

/// Rotate `models` for round-robin given how often the combo was used.
#[must_use]
pub fn rotate<T: Clone>(models: &[T], uses: u64, sticky_limit: u32) -> Vec<T> {
    if models.is_empty() {
        return Vec::new();
    }
    let len = models.len() as u64;
    let start = usize::try_from((uses / u64::from(sticky_limit.max(1))) % len).unwrap_or(0);
    models[start..]
        .iter()
        .chain(&models[..start])
        .cloned()
        .collect()
}

/// Weighted random order without replacement. `draw` returns values in
/// `[0, 1)`; zero weights go last in their original order.
#[must_use]
pub fn weighted_order<T: Clone>(
    items: &[T],
    weights: &[u32],
    mut draw: impl FnMut() -> f64,
) -> Vec<T> {
    let mut pool: Vec<(T, u32)> = items
        .iter()
        .enumerate()
        .map(|(i, t)| (t.clone(), weights.get(i).copied().unwrap_or(1)))
        .collect();
    let mut out = Vec::with_capacity(pool.len());
    loop {
        let total: u64 = pool.iter().map(|(_, w)| u64::from(*w)).sum();
        if total == 0 {
            break;
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let mut pick = (draw().clamp(0.0, 0.999_999) * total as f64) as u64;
        let index = pool
            .iter()
            .position(|(_, w)| {
                if pick < u64::from(*w) {
                    true
                } else {
                    pick -= u64::from(*w);
                    false
                }
            })
            .unwrap_or(0);
        out.push(pool.remove(index).0);
    }
    out.extend(pool.into_iter().map(|(t, _)| t));
    out
}

/// Stable sort by a key where `None` (unknown) goes last. The key is
/// computed once per item (keys may be pricing lookups).
pub fn sort_known<T, K: PartialOrd>(items: &mut Vec<T>, key: impl Fn(&T) -> Option<K>) {
    let mut keyed: Vec<(Option<K>, T)> = items.drain(..).map(|t| (key(&t), t)).collect();
    keyed.sort_by(|(a, _), (b, _)| match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    items.extend(keyed.into_iter().map(|(_, t)| t));
}

/// Input kinds in the last user turn that need special model support.
#[must_use]
pub fn required_caps(openai: &Value) -> Vec<&'static str> {
    let last_user = openai
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"));
    let mut caps = Vec::new();
    let mut need = |cap: &'static str| {
        if !caps.contains(&cap) {
            caps.push(cap);
        }
    };
    for part in last_user
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match part.get("type").and_then(Value::as_str) {
            Some("image_url") => {
                let url = part
                    .pointer("/image_url/url")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if url.starts_with("data:application/pdf")
                    || url.to_ascii_lowercase().ends_with(".pdf")
                {
                    need("pdf");
                } else {
                    need("vision");
                }
            }
            Some("file") => need("pdf"),
            Some("input_audio") => need("audio"),
            Some("video_url" | "input_video") => need("video"),
            _ => {}
        }
    }
    if openai
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty())
    {
        need("tools");
    }
    caps
}

/// Stable sort: models supporting every needed capability first, those
/// supporting some next, the rest last. Nothing is dropped.
pub fn reorder_by_caps<T>(models: &mut [T], needed: &[&str], caps_of: impl Fn(&T) -> Vec<String>) {
    if needed.is_empty() {
        return;
    }
    models.sort_by_cached_key(|model| {
        let caps = caps_of(model);
        let have = needed
            .iter()
            .filter(|n| caps.iter().any(|c| c == *n))
            .count();
        if have == needed.len() {
            0
        } else if have > 0 {
            1
        } else {
            2
        }
    });
}

/// Backoff constants: `min(base·2^(level−1), max)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooldownPolicy {
    pub base: Duration,
    pub max: Duration,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(1),
            max: Duration::from_secs(240),
        }
    }
}

impl CooldownPolicy {
    /// Cooldown for a backoff level (1-based).
    #[must_use]
    pub fn delay(&self, level: u32) -> Duration {
        let factor = 2u32.saturating_pow(level.saturating_sub(1).min(31));
        self.base.saturating_mul(factor).min(self.max)
    }
}

#[derive(Debug, Clone, Default)]
struct AccountState {
    rate_limited_until: Option<Instant>,
    backoff_level: u32,
    last_error: String,
    model_locks: HashMap<String, Instant>,
}

/// Cooldown state of one account, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CooldownInfo {
    pub connection_id: String,
    pub remaining_ms: u64,
    pub backoff_level: u32,
    pub last_error: String,
}

/// Shared, in-process gateway state: account cooldowns, round-robin counters
/// and smart-combo sticky choices (a cache of what the store persists).
#[derive(Debug, Default)]
pub struct GatewayState {
    pub policy: CooldownPolicy,
    accounts: Mutex<HashMap<String, AccountState>>,
    uses: Mutex<HashMap<String, u64>>,
    sticky: Mutex<HashMap<(String, String), Sticky>>,
    pub breakers: Breakers,
    /// Combo → label of the model that last answered (`lkgp`).
    last_good: Mutex<HashMap<String, String>>,
    /// Tool-call id → `reasoning_details` to send back with it.
    reasoning: Mutex<ReasoningMemory>,
    rng: Mutex<u64>,
}

/// How many tool calls' reasoning blocks are remembered.
const REASONING_MEMORY: usize = 512;

/// Tool call (id, name, arguments) → `reasoning_details`; oldest evicted
/// first. The name and arguments guard against made-up ids (`call_0`) that
/// repeat across conversations.
#[derive(Debug, Default)]
struct ReasoningMemory {
    entries: HashMap<String, Value>,
    order: std::collections::VecDeque<String>,
}

/// Memory key of one tool call: OpenAI-shaped `{id, function: {name, arguments}}`.
#[must_use]
pub fn tool_call_key(call: &Value) -> Option<String> {
    let id = call.get("id").and_then(Value::as_str)?;
    let name = call
        .pointer("/function/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let args = call
        .pointer("/function/arguments")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(format!("{id}\u{0}{name}\u{0}{args}"))
}

impl GatewayState {
    #[must_use]
    pub fn new(policy: CooldownPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_breaker_policy(mut self, policy: super::breaker::BreakerPolicy) -> Self {
        self.breakers = Breakers::new(policy);
        self
    }

    /// The app-wide state (cooldowns outlive single requests and tasks).
    #[must_use]
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<GatewayState>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(Self::default())))
    }

    /// Whether an account (and model on it) may be used now.
    #[must_use]
    pub fn available(&self, connection_id: &str, model: &str) -> bool {
        let now = Instant::now();
        let accounts = self.accounts.lock().unwrap_or_else(PoisonError::into_inner);
        accounts.get(connection_id).is_none_or(|state| {
            state.rate_limited_until.is_none_or(|until| until <= now)
                && state
                    .model_locks
                    .get(model)
                    .is_none_or(|until| *until <= now)
        })
    }

    /// Record a failure; returns the cooldown applied. With `model`, only that
    /// model is locked on the account; otherwise the whole account.
    pub fn penalize(&self, connection_id: &str, model: Option<&str>, reason: &str) -> Duration {
        let mut accounts = self.accounts.lock().unwrap_or_else(PoisonError::into_inner);
        let state = accounts.entry(connection_id.to_string()).or_default();
        state.backoff_level = state.backoff_level.saturating_add(1);
        state.last_error = reason.to_string();
        let delay = self.policy.delay(state.backoff_level);
        let until = Instant::now() + delay;
        match model {
            Some(model) => {
                state.model_locks.insert(model.to_string(), until);
            }
            None => state.rate_limited_until = Some(until),
        }
        delay
    }

    /// A success clears the account's backoff and the model's lock.
    pub fn succeed(&self, connection_id: &str, model: &str) {
        let mut accounts = self.accounts.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(state) = accounts.get_mut(connection_id) {
            state.backoff_level = 0;
            state.rate_limited_until = None;
            state.model_locks.remove(model);
        }
    }

    /// Accounts currently cooling down.
    #[must_use]
    pub fn cooldowns(&self) -> Vec<CooldownInfo> {
        let now = Instant::now();
        let accounts = self.accounts.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out: Vec<CooldownInfo> = accounts
            .iter()
            .filter_map(|(id, state)| {
                let latest = state
                    .model_locks
                    .values()
                    .chain(state.rate_limited_until.iter())
                    .max()
                    .copied()?;
                (latest > now).then(|| CooldownInfo {
                    connection_id: id.clone(),
                    remaining_ms: u64::try_from((latest - now).as_millis()).unwrap_or(u64::MAX),
                    backoff_level: state.backoff_level,
                    last_error: state.last_error.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        out
    }

    /// Count one use of a combo; returns the count before this use.
    pub fn next_use(&self, combo: &str) -> u64 {
        let mut uses = self.uses.lock().unwrap_or_else(PoisonError::into_inner);
        let counter = uses.entry(combo.to_string()).or_default();
        let before = *counter;
        *counter += 1;
        before
    }

    /// A value in `[0, 1)` (xorshift seeded from the clock; tests can seed).
    pub fn draw(&self) -> f64 {
        let mut state = self.rng.lock().unwrap_or_else(PoisonError::into_inner);
        if *state == 0 {
            *state = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| {
                    u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(1)
                })
                | 1;
        }
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        #[allow(clippy::cast_precision_loss)]
        let value = (x >> 11) as f64 / (1u64 << 53) as f64;
        value
    }

    /// Seed the random draws (tests).
    pub fn seed(&self, seed: u64) {
        *self.rng.lock().unwrap_or_else(PoisonError::into_inner) = seed | 1;
    }

    #[must_use]
    pub fn last_good(&self, combo: &str) -> Option<String> {
        let map = self
            .last_good
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        map.get(combo).cloned()
    }

    pub fn set_last_good(&self, combo: &str, label: &str) {
        let mut map = self
            .last_good
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        map.insert(combo.to_string(), label.to_string());
    }

    /// Keep `details` for the answer's tool calls (keys from
    /// [`tool_call_key`]; the oldest are dropped beyond the limit).
    pub fn remember_reasoning(&self, call_keys: &[String], details: &Value) {
        let mut memory = self
            .reasoning
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for key in call_keys {
            if memory
                .entries
                .insert(key.clone(), details.clone())
                .is_none()
            {
                memory.order.push_back(key.clone());
            }
        }
        while memory.order.len() > REASONING_MEMORY {
            if let Some(old) = memory.order.pop_front() {
                memory.entries.remove(&old);
            }
        }
    }

    /// Put remembered `reasoning_details` back on assistant messages that
    /// carry tool calls but lost them (the agent keeps only text and calls).
    pub fn restore_reasoning(&self, body: &mut Value) {
        let memory = self
            .reasoning
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if memory.entries.is_empty() {
            return;
        }
        for message in body
            .get_mut("messages")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if message.get("reasoning_details").is_some() {
                continue;
            }
            let found = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .and_then(|calls| {
                    calls
                        .iter()
                        .filter_map(tool_call_key)
                        .find_map(|key| memory.entries.get(&key))
                })
                .cloned();
            if let Some(details) = found {
                message["reasoning_details"] = details;
            }
        }
    }

    /// Cached sticky choice of `session` for `combo`.
    #[must_use]
    pub fn sticky(&self, session: &str, combo: &str) -> Option<Sticky> {
        let sticky = self.sticky.lock().unwrap_or_else(PoisonError::into_inner);
        sticky
            .get(&(session.to_string(), combo.to_string()))
            .cloned()
    }

    pub fn set_sticky(&self, session: &str, combo: &str, value: Sticky) {
        let mut sticky = self.sticky.lock().unwrap_or_else(PoisonError::into_inner);
        sticky.insert((session.to_string(), combo.to_string()), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn combos_parse_with_defaults() {
        let combo = Combo::from_row(&ComboRow {
            id: "k".into(),
            name: "fast".into(),
            kind: "llm".into(),
            models: r#"["a/x","b/y"]"#.into(),
            strategy: Some("round-robin".into()),
            sticky_limit: Some(0),
            confidence_threshold: None,
            weights: None,
        })
        .expect("combo");
        assert_eq!(combo.models, ["a/x", "b/y"]);
        assert_eq!(combo.strategy, Some(Strategy::RoundRobin));
        assert_eq!(combo.sticky_limit, 1);
        assert!((combo.confidence_threshold - DEFAULT_THRESHOLD).abs() < 1e-9);
        assert_eq!(Strategy::parse("smart"), Some(Strategy::Smart));
        assert_eq!(Strategy::parse("fusion"), None);
    }

    #[test]
    fn round_robin_rotates_after_sticky_limit() {
        let models = ["a", "b", "c"];
        assert_eq!(rotate(&models, 0, 1), ["a", "b", "c"]);
        assert_eq!(rotate(&models, 1, 1), ["b", "c", "a"]);
        assert_eq!(rotate(&models, 4, 1), ["b", "c", "a"]);
        assert_eq!(rotate(&models, 1, 2), ["a", "b", "c"]);
        assert_eq!(rotate(&models, 2, 2), ["b", "c", "a"]);
        assert_eq!(rotate::<&str>(&[], 3, 1), Vec::<&str>::new());
    }

    #[test]
    fn capability_reorder_is_stable_and_drops_nothing() {
        let body = json!({"messages": [
            {"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,x"}}]},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": [
                {"type": "text", "text": "and this"},
                {"type": "image_url", "image_url": {"url": "https://x/y.png"}},
                {"type": "file", "file": {"file_data": "data:application/pdf;base64,x"}}
            ]}
        ]});
        let needed = required_caps(&body);
        assert_eq!(needed, ["vision", "pdf"]);
        let mut models = vec!["text-only", "vision-a", "both-a", "text-2", "both-b"];
        reorder_by_caps(&mut models, &needed, |m| match *m {
            "vision-a" => vec!["vision".into()],
            "both-a" | "both-b" => vec!["vision".into(), "pdf".into()],
            _ => vec![],
        });
        assert_eq!(
            models,
            ["both-a", "both-b", "vision-a", "text-only", "text-2"]
        );
        let none: Vec<&str> =
            required_caps(&json!({"messages": [{"role": "user", "content": "hi"}]}));
        assert_eq!(none, Vec::<&str>::new());
    }

    #[test]
    fn backoff_grows_exponentially_up_to_the_cap() {
        let policy = CooldownPolicy::default();
        assert_eq!(policy.delay(1), Duration::from_secs(1));
        assert_eq!(policy.delay(2), Duration::from_secs(2));
        assert_eq!(policy.delay(3), Duration::from_secs(4));
        assert_eq!(policy.delay(9), Duration::from_secs(240));
        assert_eq!(policy.delay(100), Duration::from_secs(240));
    }

    #[test]
    fn cooldown_state_tracks_accounts_and_models() {
        let state = GatewayState::new(CooldownPolicy {
            base: Duration::from_secs(60),
            max: Duration::from_secs(600),
        });
        assert!(state.available("c1", "m"));
        assert_eq!(
            state.penalize("c1", Some("m"), "429"),
            Duration::from_secs(60)
        );
        assert!(!state.available("c1", "m"));
        assert!(state.available("c1", "other"), "model lock only");
        assert_eq!(state.penalize("c1", None, "401"), Duration::from_secs(120));
        assert!(!state.available("c1", "other"));
        let info = state.cooldowns();
        assert_eq!(info[0].backoff_level, 2);
        assert_eq!(info[0].last_error, "401");
        state.succeed("c1", "m");
        assert!(state.available("c1", "m"));
        assert_eq!(state.cooldowns(), Vec::new());
        assert_eq!(state.next_use("k"), 0);
        assert_eq!(state.next_use("k"), 1);
    }

    #[test]
    fn weighted_order_follows_the_draws_and_weights() {
        let models = ["a", "b", "c"];
        // Total 4 (a=1, b=3, c=0): 0.1·4 = 0 → a; then b; c (weight 0) last.
        let mut draws = [0.1, 0.5].into_iter();
        assert_eq!(
            weighted_order(&models, &[1, 3, 0], || draws.next().unwrap_or(0.0)),
            ["a", "b", "c"]
        );
        let mut draws = [0.5, 0.0].into_iter();
        assert_eq!(
            weighted_order(&models, &[1, 3, 0], || draws.next().unwrap_or(0.0)),
            ["b", "a", "c"]
        );
        // Missing weights count as 1; heavier models lead more often.
        let state = GatewayState::default();
        state.seed(42);
        let firsts = (0..400)
            .filter(|_| weighted_order(&models, &[1, 9], || state.draw())[0] == "b")
            .count();
        assert!(firsts > 250, "b (weight 9 of 11) led {firsts}/400");
    }

    #[test]
    fn unknown_values_sort_last() {
        let mut prices = vec![
            ("x", None),
            ("cheap", Some(0.1)),
            ("dear", Some(5.0)),
            ("y", None),
        ];
        sort_known(&mut prices, |p| p.1);
        assert_eq!(
            prices.iter().map(|p| p.0).collect::<Vec<_>>(),
            ["cheap", "dear", "x", "y"]
        );
        let mut context = vec![
            ("small", Some(8_000u64)),
            ("none", None),
            ("big", Some(1_000_000)),
        ];
        sort_known(&mut context, |c| c.1.map(std::cmp::Reverse));
        assert_eq!(
            context.iter().map(|c| c.0).collect::<Vec<_>>(),
            ["big", "small", "none"]
        );
    }

    #[test]
    fn reasoning_details_come_back_by_tool_call_id() {
        let state = GatewayState::default();
        let details = json!([{"type": "reasoning.text", "text": "t", "signature": "s"}]);
        let call = |id: &str, args: &str| json!({"id": id, "function": {"name": "read", "arguments": args}});
        let key = tool_call_key(&call("call_0", "{\"path\":\"a\"}")).expect("key");
        state.remember_reasoning(&[key], &details);
        let mut body = json!({"messages": [
            {"role": "assistant", "tool_calls": [call("call_0", "{\"path\":\"a\"}")]},
            {"role": "assistant", "tool_calls": [call("call_0", "{\"path\":\"b\"}")]},
            {"role": "assistant", "tool_calls": [call("other", "{}")]},
        ]});
        state.restore_reasoning(&mut body);
        assert_eq!(body["messages"][0]["reasoning_details"], details);
        assert!(
            body["messages"][1].get("reasoning_details").is_none(),
            "same id, other call"
        );
        assert!(body["messages"][2].get("reasoning_details").is_none());
        // Bounded: the oldest entries go first.
        for i in 0..=REASONING_MEMORY {
            state.remember_reasoning(&[format!("k{i}")], &details);
        }
        let memory = state.reasoning.lock().expect("lock");
        assert_eq!(memory.entries.len(), REASONING_MEMORY);
        assert!(
            !memory.entries.contains_key("k0")
                && memory.entries.contains_key(&format!("k{REASONING_MEMORY}"))
        );
        state.set_last_good("k", "a/x");
        assert_eq!(state.last_good("k").as_deref(), Some("a/x"));
    }
}
