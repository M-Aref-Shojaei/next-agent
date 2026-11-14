// Design adapted from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/rtk/index.js, open-sse/rtk/systemInject.js @ 1855aa4
//! Token savers, run on the provider-format body after translation, in
//! 9Router's order: **RTK → Caveman/Ponytail**, then Anthropic prompt-cache
//! breakpoints. Every step is fail-open: if it errors or panics, the body
//! stays as it was before that step.

pub mod prompts;
pub mod rtk;

use prompts::Level;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Body format the savers operate on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Which savers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaverConfig {
    /// Global switch; off = nothing runs.
    pub enabled: bool,
    pub caveman: Option<Level>,
    pub ponytail: Option<Level>,
    pub rtk: bool,
    pub cache_control: bool,
}

impl Default for SaverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            caveman: None,
            ponytail: None,
            rtk: true,
            cache_control: true,
        }
    }
}

/// What the savers did to one request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaverReport {
    /// Bytes of the tool results RTK compressed, before and after.
    pub bytes_before: usize,
    pub bytes_after: usize,
    /// RTK filter name → results it compressed.
    pub hits: BTreeMap<&'static str, usize>,
    /// Prompts injected (`caveman`, `ponytail`).
    pub injected: Vec<&'static str>,
    pub cache_breakpoints: usize,
}

/// Run the enabled savers on `body`.
pub fn apply(body: &mut Value, format: Format, config: &SaverConfig) -> SaverReport {
    let mut report = SaverReport::default();
    if !config.enabled {
        return report;
    }
    if config.rtk {
        fail_open(body, |b| {
            compress_tool_results(b, format, &mut report);
        });
    }
    for (name, level, make) in [
        (
            "caveman",
            config.caveman,
            prompts::caveman_prompt as fn(Level) -> String,
        ),
        ("ponytail", config.ponytail, prompts::ponytail_prompt),
    ] {
        if let Some(level) = level {
            let prompt = make(level);
            let mut changed = false;
            fail_open(body, |b| changed = prompts::inject(b, format, &prompt));
            if changed {
                report.injected.push(name);
            }
        }
    }
    if config.cache_control && format == Format::Anthropic {
        let mut added = 0;
        fail_open(body, |b| added = add_cache_breakpoints(b));
        report.cache_breakpoints = added;
    }
    report
}

/// Run `step` on a copy; keep the copy only if the step did not panic.
fn fail_open(body: &mut Value, step: impl FnOnce(&mut Value)) {
    let mut copy = body.clone();
    if catch_unwind(AssertUnwindSafe(|| step(&mut copy))).is_ok() {
        *body = copy;
    } else {
        tracing::warn!(target: "router", "token saver failed; request sent unchanged");
    }
}

fn compress_text(text: &mut String, report: &mut SaverReport) {
    if let Some((out, filter)) = rtk::compress(text) {
        report.bytes_before += text.len();
        report.bytes_after += out.len();
        *report.hits.entry(filter.as_str()).or_default() += 1;
        *text = out;
    }
}

fn compress_tool_results(body: &mut Value, format: Format, report: &mut SaverReport) {
    match format {
        Format::OpenAi => {
            for message in body
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if message["role"] != "tool" || message.get("is_error") == Some(&Value::Bool(true))
                {
                    continue;
                }
                if let Some(Value::String(text)) = message.get_mut("content") {
                    compress_text(text, report);
                }
            }
        }
        Format::Anthropic => {
            for message in body
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                for block in message
                    .get_mut("content")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    if block["type"] != "tool_result"
                        || block.get("is_error") == Some(&Value::Bool(true))
                    {
                        continue;
                    }
                    match block.get_mut("content") {
                        Some(Value::String(text)) => compress_text(text, report),
                        Some(Value::Array(parts)) => {
                            for part in parts {
                                if let Some(Value::String(text)) = part.get_mut("text") {
                                    compress_text(text, report);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Format::Gemini => {
            for content in body
                .get_mut("contents")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                for part in content
                    .get_mut("parts")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    if let Some(Value::String(text)) =
                        part.pointer_mut("/functionResponse/response/content")
                    {
                        compress_text(text, report);
                    }
                }
            }
        }
    }
}

fn mark(block: &mut Value) -> bool {
    match block.as_object_mut() {
        Some(map) if !map.contains_key("cache_control") => {
            map.insert("cache_control".into(), json!({"type": "ephemeral"}));
            true
        }
        _ => false,
    }
}

/// Anthropic prompt caching: mark the system text, the last tool and the last
/// block of the last assistant message. Returns how many marks were added.
pub fn add_cache_breakpoints(body: &mut Value) -> usize {
    let mut added = 0;
    if let Some(Value::String(text)) = body.get("system").cloned() {
        if !text.is_empty() {
            body["system"] = json!([{"type": "text", "text": text}]);
        }
    }
    if let Some(last) = body
        .get_mut("system")
        .and_then(Value::as_array_mut)
        .and_then(|blocks| blocks.last_mut())
    {
        added += usize::from(mark(last));
    }
    if let Some(last) = body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.last_mut())
    {
        added += usize::from(mark(last));
    }
    if let Some(message) = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|messages| messages.iter_mut().rev().find(|m| m["role"] == "assistant"))
    {
        if let Some(Value::String(text)) = message.get("content").cloned() {
            message["content"] = json!([{"type": "text", "text": text}]);
        }
        if let Some(last) = message
            .get_mut("content")
            .and_then(Value::as_array_mut)
            .and_then(|blocks| blocks.last_mut())
        {
            added += usize::from(mark(last));
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    fn grep_output() -> String {
        let mut s = String::new();
        for i in 0..30 {
            let _ = writeln!(s, "src/a.rs:{i}:    let value_{i} = compute();");
        }
        s
    }

    #[test]
    fn rtk_compresses_openai_tool_messages_only() {
        let big = grep_output();
        let mut body = json!({"messages": [
            {"role": "user", "content": big.clone()},
            {"role": "tool", "tool_call_id": "c", "content": big.clone()},
            {"role": "tool", "tool_call_id": "d", "content": big.clone(), "is_error": true}
        ]});
        let report = apply(&mut body, Format::OpenAi, &SaverConfig::default());
        assert_eq!(body["messages"][0]["content"], big, "user text untouched");
        assert_ne!(body["messages"][1]["content"], big);
        assert_eq!(body["messages"][2]["content"], big, "errors untouched");
        assert_eq!(report.hits.get("grep"), Some(&1));
        assert!(report.bytes_after < report.bytes_before);
    }

    #[test]
    fn rtk_handles_anthropic_and_gemini_results() {
        let big = grep_output();
        let mut anthropic = json!({"messages": [{"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "a", "content": big.clone()},
            {"type": "tool_result", "tool_use_id": "b", "content": [{"type": "text", "text": big.clone()}]}
        ]}]});
        let report = apply(
            &mut anthropic,
            Format::Anthropic,
            &SaverConfig {
                cache_control: false,
                ..SaverConfig::default()
            },
        );
        assert_eq!(report.hits.get("grep"), Some(&2));
        let mut gemini = json!({"contents": [{"role": "user", "parts": [
            {"functionResponse": {"name": "grep", "response": {"content": big}}}
        ]}]});
        assert_eq!(
            apply(&mut gemini, Format::Gemini, &SaverConfig::default())
                .hits
                .get("grep"),
            Some(&1)
        );
    }

    #[test]
    fn disabled_savers_do_nothing() {
        let mut body = json!({"messages": [{"role": "tool", "content": grep_output()}]});
        let before = body.clone();
        let config = SaverConfig {
            enabled: false,
            caveman: Some(Level::Full),
            ..SaverConfig::default()
        };
        assert_eq!(
            apply(&mut body, Format::OpenAi, &config),
            SaverReport::default()
        );
        assert_eq!(body, before);
    }

    #[test]
    fn caveman_then_cache_is_idempotent_for_anthropic() {
        let config = SaverConfig {
            caveman: Some(Level::Lite),
            ponytail: Some(Level::Full),
            ..SaverConfig::default()
        };
        let mut body = json!({
            "system": "Base prompt.",
            "tools": [{"name": "a", "input_schema": {}}, {"name": "b", "input_schema": {}}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": [{"type": "text", "text": "more"}]}
            ]
        });
        let report = apply(&mut body, Format::Anthropic, &config);
        assert_eq!(report.injected, ["caveman", "ponytail"]);
        assert_eq!(report.cache_breakpoints, 3);
        assert!(body["system"][0]["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Base prompt.\n\nToken saver")));
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        let once = body.clone();
        let again = apply(&mut body, Format::Anthropic, &config);
        assert_eq!(again.injected, Vec::<&str>::new());
        assert_eq!(again.cache_breakpoints, 0);
        assert_eq!(body, once, "second pass changes nothing");
    }

    #[test]
    fn cache_breakpoints_only_for_anthropic() {
        let mut body = json!({"messages": [{"role": "system", "content": "x"}]});
        let report = apply(&mut body, Format::OpenAi, &SaverConfig::default());
        assert_eq!(report.cache_breakpoints, 0);
    }

    #[test]
    fn failing_steps_leave_the_body_unchanged() {
        let mut body = json!({"a": 1});
        let before = body.clone();
        fail_open(&mut body, |b| {
            b["a"] = json!(2);
            panic!("boom");
        });
        assert_eq!(body, before);
    }
}
