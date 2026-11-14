// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/services/combo.js, open-sse/services/accountFallback.js @ 1855aa4
//! Combos (named model lists with a strategy), capability reordering and
//! per-account cooldown with exponential backoff.
//!
//! Strategies: `fallback` (in order) and `round-robin` (rotate the start,
//! advancing after `sticky_limit` uses). 9Router's `smart` and `fusion` are
//! not ported.

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
}

impl Strategy {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "fallback" => Some(Self::Fallback),
            "round-robin" => Some(Self::RoundRobin),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::RoundRobin => "round-robin",
        }
    }
}

/// A named list of `provider/model` targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    /// Per-combo strategy; `None` = the global setting.
    pub strategy: Option<Strategy>,
    /// Round-robin: uses before advancing (≥ 1).
    pub sticky_limit: u32,
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

/// Input kinds in the last user turn that need special model support.
#[must_use]
pub fn required_caps(openai: &Value) -> Vec<&'static str> {
    let Some(last_user) = openai
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
    else {
        return Vec::new();
    };
    let mut caps = Vec::new();
    let mut need = |cap: &'static str| {
        if !caps.contains(&cap) {
            caps.push(cap);
        }
    };
    for part in last_user
        .get("content")
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

/// Shared, in-process gateway state: account cooldowns and round-robin counters.
#[derive(Debug, Default)]
pub struct GatewayState {
    pub policy: CooldownPolicy,
    accounts: Mutex<HashMap<String, AccountState>>,
    uses: Mutex<HashMap<String, u64>>,
}

impl GatewayState {
    #[must_use]
    pub fn new(policy: CooldownPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
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
        })
        .expect("combo");
        assert_eq!(combo.models, ["a/x", "b/y"]);
        assert_eq!(combo.strategy, Some(Strategy::RoundRobin));
        assert_eq!(combo.sticky_limit, 1);
        assert_eq!(Strategy::parse("smart"), None);
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
}
