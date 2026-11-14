//! The `smart` combo strategy: one sticky model per session (task).
//!
//! 9Router names this strategy and its 0.7 / 0.8 / 0.9 confidence thresholds,
//! but its quality signal was not captured in our research, so the signal
//! below is our own design.
//!
//! A session keeps the model that last answered. Each request tries that model
//! first, then the rest of the combo in order after it (the usual fall-through
//! rules apply). The sticky model changes only when:
//! - **failover**: another model had to answer because the sticky one failed;
//! - **quality**: confidence drops below the combo's threshold.
//!
//! **Quality signal.** Every answer the sticky model gives is scored good or
//! bad. Bad means one of:
//! - empty: no text and no tool calls;
//! - refused: `content_filter`, or a short text-only reply that opens with a
//!   refusal phrase ("I can't help with…");
//! - bad tool JSON: a tool call whose arguments do not parse as JSON;
//! - truncated: `finish_reason = length`.
//!
//! Confidence is the share of good answers over the last [`WINDOW`] answers,
//! where answers not yet seen count as good (a fresh model starts at 1.0, so
//! one early slip cannot switch it). With `WINDOW = 10`, the model switches on
//! the 2nd bad answer in the window at threshold 0.9, the 3rd at 0.8 and the
//! 4th at 0.7. A switch moves to the next model in combo order and starts a
//! fresh window.

use crate::llm::Completion;

/// Answers the confidence is measured over.
pub const WINDOW: usize = 10;
/// Threshold for combos that do not set one.
pub const DEFAULT_THRESHOLD: f64 = 0.8;

/// Opening phrases of a refusal (matched lower-case at the start of the reply).
const REFUSALS: [&str; 10] = [
    "i can't help",
    "i cannot help",
    "i can't assist",
    "i cannot assist",
    "i'm sorry, but i can",
    "i am sorry, but i can",
    "i'm unable to",
    "i am unable to",
    "i won't be able to",
    "sorry, i can't",
];
/// Longer replies are treated as real answers even if they open politely.
const REFUSAL_MAX_CHARS: usize = 400;

/// How one answer scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Good,
    Empty,
    Refused,
    BadToolJson,
    Truncated,
}

impl Signal {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Empty => "empty",
            Self::Refused => "refused",
            Self::BadToolJson => "bad-tool-json",
            Self::Truncated => "truncated",
        }
    }
}

/// Score one completion.
#[must_use]
pub fn score(completion: &Completion) -> Signal {
    let text = completion.content.trim();
    if completion.finish_reason.as_deref() == Some("length") {
        return Signal::Truncated;
    }
    if completion.finish_reason.as_deref() == Some("content_filter") {
        return Signal::Refused;
    }
    if text.is_empty() && completion.tool_calls.is_empty() {
        return Signal::Empty;
    }
    let bad_args = completion.tool_calls.iter().any(|call| {
        let args = call.function.arguments.trim();
        !args.is_empty() && serde_json::from_str::<serde::de::IgnoredAny>(args).is_err()
    });
    if bad_args {
        return Signal::BadToolJson;
    }
    if completion.tool_calls.is_empty() && text.chars().nth(REFUSAL_MAX_CHARS).is_none() {
        // Only the opening matters; the longest phrase is under 32 chars.
        let lower: String = text
            .chars()
            .take(32)
            .flat_map(char::to_lowercase)
            .map(|c| if c == '’' { '\'' } else { c })
            .collect();
        if REFUSALS.iter().any(|p| lower.starts_with(p)) {
            return Signal::Refused;
        }
    }
    Signal::Good
}

/// The sticky choice of one session for one combo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sticky {
    /// `provider/model` label of the sticky target.
    pub model: String,
    /// Recent answers of `model`, oldest first; `true` = bad. At most
    /// [`WINDOW`]; empty while it holds no bad answer (only those count).
    pub window: Vec<bool>,
}

impl Sticky {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            window: Vec::new(),
        }
    }

    /// Good answers over the last [`WINDOW`], unseen ones counting as good.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        let bad = self.window.iter().filter(|b| **b).count();
        #[allow(clippy::cast_precision_loss)] // Both are ≤ WINDOW.
        let share = (WINDOW - bad.min(WINDOW)) as f64 / WINDOW as f64;
        share
    }

    /// Add one answer to the window.
    pub fn push(&mut self, signal: Signal) {
        self.window.push(signal != Signal::Good);
        if self.window.len() > WINDOW {
            self.window.drain(..self.window.len() - WINDOW);
        }
        if !self.window.contains(&true) {
            self.window.clear();
        }
    }
}

/// Why a session's sticky model changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    pub from: String,
    pub to: String,
    /// `failover`, or `quality:<signal>` for the answer that tipped it.
    pub reason: String,
    pub confidence: f64,
}

/// Index in `labels` (combo order) to start at: the sticky model, else 0.
#[must_use]
pub fn start_index(labels: &[String], sticky: Option<&Sticky>) -> usize {
    sticky
        .and_then(|s| labels.iter().position(|l| *l == s.model))
        .unwrap_or(0)
}

/// Update `sticky` after `answered` (a label from `combo_order`) produced
/// `completion`. Returns the switch, if any.
pub fn observe(
    sticky: &mut Option<Sticky>,
    combo_order: &[String],
    answered: &str,
    completion: &Completion,
    threshold: f64,
) -> Option<Switch> {
    let mut switch = None;
    let current = sticky.get_or_insert_with(|| Sticky::new(answered));
    if current.model != answered {
        let old = std::mem::replace(current, Sticky::new(answered)).model;
        // A sticky model removed from the combo is a reset, not a failover.
        if combo_order.contains(&old) {
            switch = Some(Switch {
                from: old,
                to: answered.to_string(),
                reason: "failover".into(),
                confidence: 1.0,
            });
        }
    }
    let signal = score(completion);
    current.push(signal);
    let confidence = current.confidence();
    if confidence < threshold && combo_order.len() > 1 {
        let next = combo_order
            .iter()
            .position(|m| m == answered)
            .map_or(0, |i| (i + 1) % combo_order.len());
        let to = combo_order[next].clone();
        *current = Sticky::new(to.clone());
        // A quality switch supersedes a failover switch in the same request.
        let from = switch.map_or_else(|| answered.to_string(), |s| s.from);
        switch = Some(Switch {
            from,
            to,
            reason: format!("quality:{}", signal.as_str()),
            confidence,
        });
    }
    switch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{FunctionCall, ToolCall};

    fn answer(text: &str, finish: Option<&str>) -> Completion {
        Completion {
            content: text.into(),
            tool_calls: Vec::new(),
            finish_reason: finish.map(str::to_string),
        }
    }

    fn tool_call(arguments: &str) -> Completion {
        Completion {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: "read".into(),
                    arguments: arguments.into(),
                },
            }],
            finish_reason: Some("tool_calls".into()),
        }
    }

    #[test]
    fn signals_classify_each_kind_of_bad_answer() {
        assert_eq!(
            score(&answer("Here is the fix.", Some("stop"))),
            Signal::Good
        );
        assert_eq!(score(&answer("  ", Some("stop"))), Signal::Empty);
        assert_eq!(score(&answer("partial", Some("length"))), Signal::Truncated);
        assert_eq!(score(&answer("", Some("content_filter"))), Signal::Refused);
        assert_eq!(
            score(&answer("I can’t help with that request.", None)),
            Signal::Refused
        );
        assert_eq!(
            score(&answer("Sorry, I can't do that.", None)),
            Signal::Refused
        );
        let long = format!("I can't help noticing… {}", "detail ".repeat(80));
        assert_eq!(
            score(&answer(&long, None)),
            Signal::Good,
            "long answers are real"
        );
        assert_eq!(score(&tool_call(r#"{"path":"a"}"#)), Signal::Good);
        assert_eq!(score(&tool_call("")), Signal::Good, "no arguments is fine");
        assert_eq!(score(&tool_call(r#"{"path":"a""#)), Signal::BadToolJson);
    }

    #[test]
    fn confidence_starts_full_and_slides() {
        let mut sticky = Sticky::new("a/x");
        assert!((sticky.confidence() - 1.0).abs() < 1e-9);
        sticky.push(Signal::Empty);
        assert!((sticky.confidence() - 0.9).abs() < 1e-9);
        for _ in 0..WINDOW {
            sticky.push(Signal::Good);
        }
        assert!(sticky.window.is_empty(), "old slips age out");
        assert!((sticky.confidence() - 1.0).abs() < 1e-9);
        for _ in 0..WINDOW + 3 {
            sticky.push(Signal::Truncated);
        }
        assert_eq!(sticky.window.len(), WINDOW, "window is capped");
    }

    /// Bad answers in a row before the model switches, per threshold.
    fn bad_answers_to_switch(threshold: f64) -> usize {
        let order = vec!["a/x".to_string(), "b/y".to_string()];
        let mut sticky = None;
        for n in 1..=WINDOW {
            if let Some(switch) = observe(&mut sticky, &order, "a/x", &answer("", None), threshold)
            {
                assert_eq!(switch.from, "a/x");
                assert_eq!(switch.to, "b/y");
                assert_eq!(switch.reason, "quality:empty");
                assert_eq!(sticky.as_ref().map(|s| s.model.as_str()), Some("b/y"));
                assert!(sticky.as_ref().is_some_and(|s| s.window.is_empty()));
                return n;
            }
        }
        panic!("never switched at {threshold}");
    }

    #[test]
    fn thresholds_set_how_many_slips_are_tolerated() {
        assert_eq!(bad_answers_to_switch(0.9), 2);
        assert_eq!(bad_answers_to_switch(0.8), 3);
        assert_eq!(bad_answers_to_switch(0.7), 4);
    }

    #[test]
    fn good_answers_keep_the_model_and_failover_moves_it() {
        let order = vec!["a/x".to_string(), "b/y".to_string(), "c/z".to_string()];
        let mut sticky = None;
        let good = answer("ok", Some("stop"));
        assert_eq!(observe(&mut sticky, &order, "a/x", &good, 0.9), None);
        assert_eq!(observe(&mut sticky, &order, "a/x", &good, 0.9), None);
        let switch = observe(&mut sticky, &order, "b/y", &good, 0.9).expect("failover");
        assert_eq!((switch.from.as_str(), switch.to.as_str()), ("a/x", "b/y"));
        assert_eq!(switch.reason, "failover");
        assert_eq!(sticky.as_ref().map(|s| s.model.as_str()), Some("b/y"));
        // A sticky model no longer in the combo is replaced without a switch.
        let mut sticky = Some(Sticky::new("gone/m"));
        assert_eq!(observe(&mut sticky, &order, "a/x", &good, 0.9), None);
        assert_eq!(sticky.as_ref().map(|s| s.model.as_str()), Some("a/x"));
        // Quality switch from the last model wraps to the first.
        let mut sticky = Some(Sticky {
            model: "c/z".into(),
            window: vec![true],
        });
        let switch = observe(
            &mut sticky,
            &order,
            "c/z",
            &answer("x", Some("length")),
            0.9,
        )
        .expect("switch");
        assert_eq!(switch.to, "a/x");
        assert_eq!(switch.reason, "quality:truncated");
        // A single-model combo never switches on quality.
        let solo = vec!["a/x".to_string()];
        let mut sticky = None;
        for _ in 0..WINDOW {
            assert_eq!(
                observe(&mut sticky, &solo, "a/x", &answer("", None), 0.9),
                None
            );
        }
    }

    #[test]
    fn start_index_finds_the_sticky_model() {
        let labels = ["a".to_string(), "b".to_string()];
        assert_eq!(start_index(&labels, Some(&Sticky::new("b"))), 1);
        assert_eq!(start_index(&labels, Some(&Sticky::new("gone"))), 0);
        assert_eq!(start_index(&labels, None), 0);
    }
}
