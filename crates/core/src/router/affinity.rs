// Ported from OmniRoute (MIT, © 2026 diegosouzapw): open-sse/services/combo/promptCacheAffinity.ts @ v3.8.53
//! Prompt-cache affinity: keep one conversation on one account so the
//! provider's prompt cache stays warm.
//!
//! The key is the request's `prompt_cache_key`, else a hash of its stable
//! prefix (system messages, tools and the first user message). Each account
//! scores `0.75·hash(key‖account) + 0.25·availability` (rendezvous hashing),
//! highest first. The same key always ranks accounts the same way, and adding
//! or removing an account only moves the conversations that were on it. The
//! gateway ranks only within accounts of equal priority, so the user's
//! primary/backup order still holds.

use crate::hash::sha256_hex;
use serde_json::{json, Value};

/// Share of the score that comes from the key/target hash.
const HASH_WEIGHT: f64 = 0.75;
/// Share of the score that comes from target availability.
const AVAILABILITY_WEIGHT: f64 = 0.25;

/// Affinity key of a request, if it has a usable prefix.
#[must_use]
pub fn key(body: &Value) -> Option<String> {
    if let Some(explicit) = body.get("prompt_cache_key").and_then(Value::as_str) {
        return Some(explicit.to_string());
    }
    let messages = body.get("messages").and_then(Value::as_array)?;
    let system: Vec<&Value> = messages
        .iter()
        .take_while(|m| m["role"] == "system" || m["role"] == "developer")
        .collect();
    let first_user = messages.iter().find(|m| m["role"] == "user");
    if system.is_empty() && first_user.is_none() {
        return None;
    }
    let prefix = json!({
        "system": system,
        "tools": body.get("tools"),
        "user": first_user,
    });
    Some(sha256_hex(prefix.to_string().as_bytes()))
}

/// Deterministic value in `[0, 1)` for `key` and `target`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn hash01(key: &str, target: &str) -> f64 {
    let digest = sha256_hex(format!("{key}\u{0}{target}").as_bytes());
    // 52 bits (13 hex digits) → an exact f64 in [0, 1).
    let bits = u64::from_str_radix(&digest[..13], 16).unwrap_or(0);
    bits as f64 / (1u64 << 52) as f64
}

/// Sort `items` by affinity score, highest first. `id` names a target and
/// `availability` is in `[0, 1]` (1 = healthy).
pub fn rank<T>(
    items: &mut [T],
    key: &str,
    id: impl Fn(&T) -> String,
    availability: impl Fn(&T) -> f64,
) {
    items.sort_by_cached_key(|item| {
        let score = HASH_WEIGHT * hash01(key, &id(item)) + AVAILABILITY_WEIGHT * availability(item);
        // Non-negative floats order like their bit patterns.
        std::cmp::Reverse(score.max(0.0).to_bits())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_come_from_the_stable_prefix() {
        let base = json!({"messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "fix the bug"}
        ]});
        let mut later = base.clone();
        later["messages"]
            .as_array_mut()
            .expect("array")
            .push(json!({"role": "assistant", "content": "done"}));
        assert_eq!(key(&base), key(&later), "later turns do not change the key");
        let other = json!({"messages": [{"role": "user", "content": "another task"}]});
        assert_ne!(key(&base), key(&other));
        assert_eq!(
            key(&json!({"prompt_cache_key": "conv-1", "messages": []})).as_deref(),
            Some("conv-1")
        );
        assert_eq!(key(&json!({"messages": []})), None);
    }

    #[test]
    fn ranking_is_stable_and_spreads_conversations() {
        let accounts = ["a", "b", "c"];
        let first = |key: &str| {
            let mut list = accounts;
            rank(&mut list, key, |s| (*s).to_string(), |_| 1.0);
            list[0]
        };
        assert_eq!(first("conv-1"), first("conv-1"), "same key, same account");
        let picks: std::collections::HashSet<&str> =
            (0..40).map(|i| first(&format!("conv-{i}"))).collect();
        assert_eq!(picks.len(), 3, "different conversations use every account");
        // An unavailable account drops behind a healthy one with a close hash.
        let mut list = accounts;
        rank(
            &mut list,
            "conv-1",
            |s| (*s).to_string(),
            |s| f64::from(u8::from(*s != first("conv-1"))),
        );
        assert_ne!(list[0], first("conv-1"));
        let h = hash01("k", "t");
        assert!((0.0..1.0).contains(&h));
    }
}
