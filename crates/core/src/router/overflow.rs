// Design adapted from OpenRouter docs: https://openrouter.ai/docs/features/message-transforms
//! Context overflow switch: when the prompt will not fit a model's context
//! window, try the combo's models that fit first. We never trim the prompt;
//! shrinking it is the agent engine's compaction job.
//!
//! The prompt size is estimated as one token per four bytes of the JSON
//! messages and tools (deliberately rough; it only has to catch clear
//! overflows), plus the requested output budget.

use serde_json::Value;

/// Rough prompt + output token estimate for an OpenAI body.
#[must_use]
pub fn estimate_tokens(body: &Value) -> u64 {
    let bytes = |key: &str| body.get(key).map_or(0, |v| v.to_string().len());
    let prompt = (bytes("messages") + bytes("tools")) as u64 / 4;
    let output = body
        .get("max_completion_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    prompt + output
}

/// Stable partition: targets whose window fits `needed` (or is unknown)
/// first, too-small ones last. Nothing is dropped; returns whether the order
/// changed.
pub fn prefer_fitting<T>(
    targets: &mut Vec<T>,
    needed: u64,
    context_of: impl Fn(&T) -> Option<u64>,
) -> bool {
    let fits = |t: &T| context_of(t).is_none_or(|limit| limit >= needed);
    if targets.iter().all(&fits) || !targets.iter().any(&fits) {
        return false;
    }
    let (mut ok, small): (Vec<T>, Vec<T>) = targets.drain(..).partition(|t| fits(t));
    ok.extend(small);
    *targets = ok;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn estimates_grow_with_the_prompt_and_output_budget() {
        let small = json!({"messages": [{"role": "user", "content": "hi"}]});
        let big = json!({"messages": [{"role": "user", "content": "x".repeat(40_000)}], "max_tokens": 1000});
        assert!(estimate_tokens(&small) < 20);
        assert!((10_000..12_000).contains(&estimate_tokens(&big)));
    }

    #[test]
    fn fitting_models_move_ahead() {
        let mut targets = vec![
            ("small", Some(8_000)),
            ("unknown", None),
            ("big", Some(200_000)),
        ];
        assert!(prefer_fitting(&mut targets, 50_000, |t| t.1));
        assert_eq!(
            targets.iter().map(|t| t.0).collect::<Vec<_>>(),
            ["unknown", "big", "small"]
        );
        let mut none_fit = vec![("a", Some(1_000)), ("b", Some(2_000))];
        assert!(
            !prefer_fitting(&mut none_fit, 50_000, |t| t.1),
            "nothing fits: leave as is"
        );
        assert_eq!(none_fit[0].0, "a");
    }
}
