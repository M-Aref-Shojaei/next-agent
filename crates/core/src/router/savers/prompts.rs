// Design adapted from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/rtk/caveman.js, cavemanPrompts.js, systemInject.js, ponytail*.js @ 1855aa4
//! Caveman (terse answers) and Ponytail (minimal code) system-prompt
//! injection. The wording is our own; only the mechanism follows 9Router.
//!
//! Each prompt is a single `\n\n`-separated segment, so injection is
//! idempotent: a prompt already present as a whole segment is not added again.

use super::Format;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Strength of a style prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Lite,
    Full,
    Ultra,
}

impl Level {
    /// `lite` | `full` | `ultra`; anything else (incl. `off`) is `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "lite" => Some(Self::Lite),
            "full" => Some(Self::Full),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }
}

const CAVEMAN_SHARED: &[&str] = &[
    "- Code, commands, file paths, identifiers, error text and quotes stay exactly as they are; never abbreviate inside them.",
    "- Where a short form could be misread (warnings, destructive or irreversible steps, security notes), say it in plain full sentences.",
    "- Hold this style for the whole conversation, not just this turn.",
    "- Common short forms are fine: cfg, fn, dir, repo, deps, impl, env, w/, w/o, b/c, →.",
    "- Reply in the language the user writes in.",
    "- Do not talk about this style or about yourself; no greetings, sign-offs, emoji or decorative headings unless asked.",
];

const PONYTAIL_SHARED: &[&str] = &[
    "- Correctness first: keep the error handling, validation and security checks the code needs.",
    "- Add tests only where the task or the project's conventions call for them.",
    "- Reply in the language the user writes in.",
    "- Do not talk about this rule or about yourself.",
];

fn caveman_lines(level: Level) -> &'static [&'static str] {
    match level {
        Level::Lite => &[
            "Token saver: answer concisely.",
            "- Cut filler, pleasantries, repetition and hedging; keep normal grammar.",
            "- Lead with the answer, then only the detail that changes what the user does next.",
        ],
        Level::Full => &[
            "Token saver: answer in compact note form.",
            "- Sentence fragments are fine; drop articles, filler words and restated context.",
            "- One fact or step per line; prefer lists over paragraphs.",
            "- Skip explanations the user did not ask for.",
        ],
        Level::Ultra => &[
            "Token saver: maximum compression.",
            "- Keywords and symbols (→ = ≠ ✓ ✗) instead of sentences; no prose paragraphs.",
            "- Only what the user needs to act; no background, no alternatives unless asked.",
        ],
    }
}

fn ponytail_lines(level: Level) -> &'static [&'static str] {
    match level {
        Level::Lite => &[
            "Code style: keep it simple.",
            "- Prefer the most direct solution; avoid speculative abstractions and options nobody asked for.",
        ],
        Level::Full => &[
            "Code style: write the least code that does the job (YAGNI).",
            "- No extra features, flags, layers or generic helpers beyond the request.",
            "- Reuse what the project already has before adding anything new.",
            "- No defensive code for situations that cannot happen.",
        ],
        Level::Ultra => &[
            "Code style: smallest possible diff.",
            "- No new files, dependencies or helpers unless there is no other way.",
            "- Do not touch, rename or reformat code outside the change.",
            "- Comments only for what is not obvious from the code.",
        ],
    }
}

/// Caveman prompt text for a level (one `\n\n` segment).
#[must_use]
pub fn caveman_prompt(level: Level) -> String {
    caveman_lines(level)
        .iter()
        .chain(CAVEMAN_SHARED)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Ponytail prompt text for a level (one `\n\n` segment).
#[must_use]
pub fn ponytail_prompt(level: Level) -> String {
    ponytail_lines(level)
        .iter()
        .chain(PONYTAIL_SHARED)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_segment(text: &str, prompt: &str) -> bool {
    text.split("\n\n").any(|segment| segment.trim() == prompt)
}

fn append(text: &str, prompt: &str) -> String {
    if text.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n\n{prompt}", text.trim_end())
    }
}

/// Inject `prompt` into the system prompt of a request body in `format`.
/// Returns `true` if the body changed.
pub fn inject(body: &mut Value, format: Format, prompt: &str) -> bool {
    match format {
        Format::OpenAi => inject_openai(body, prompt),
        Format::Anthropic => inject_anthropic(body, prompt),
        Format::Gemini => inject_gemini(body, prompt),
    }
}

fn inject_openai(body: &mut Value, prompt: &str) -> bool {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };
    let first_system = messages
        .iter()
        .position(|m| matches!(m["role"].as_str(), Some("system" | "developer")));
    match first_system {
        Some(index) => {
            let message = &mut messages[index];
            match message.get_mut("content") {
                Some(Value::String(text)) => {
                    if has_segment(text, prompt) {
                        return false;
                    }
                    *text = append(text, prompt);
                }
                Some(Value::Array(parts)) => {
                    let present = parts.iter().any(|p| {
                        p.get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|t| has_segment(t, prompt))
                    });
                    if present {
                        return false;
                    }
                    parts.push(json!({"type": "text", "text": prompt}));
                }
                _ => message["content"] = json!(prompt),
            }
        }
        None => messages.insert(0, json!({"role": "system", "content": prompt})),
    }
    true
}

fn inject_anthropic(body: &mut Value, prompt: &str) -> bool {
    match body.get_mut("system") {
        Some(Value::String(text)) => {
            if has_segment(text, prompt) {
                return false;
            }
            *text = append(text, prompt);
        }
        Some(Value::Array(blocks)) => {
            let present = blocks.iter().any(|b| {
                b.get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|t| has_segment(t, prompt))
            });
            if present {
                return false;
            }
            // Insert before the last cached block so the cached prefix stays valid.
            let at = blocks
                .iter()
                .rposition(|b| b.get("cache_control").is_some())
                .unwrap_or(blocks.len());
            blocks.insert(at, json!({"type": "text", "text": prompt}));
        }
        _ => {
            if let Some(map) = body.as_object_mut() {
                map.insert("system".into(), json!(prompt));
            } else {
                return false;
            }
        }
    }
    true
}

fn inject_gemini(body: &mut Value, prompt: &str) -> bool {
    let Some(map) = body.as_object_mut() else {
        return false;
    };
    let system = map
        .entry("systemInstruction")
        .or_insert_with(|| json!({"parts": []}));
    if !system.get("parts").is_some_and(Value::is_array) {
        system["parts"] = json!([]);
    }
    let Some(parts) = system["parts"].as_array_mut() else {
        return false;
    };
    let present = parts.iter().any(|p| {
        p.get("text")
            .and_then(Value::as_str)
            .is_some_and(|t| has_segment(t, prompt))
    });
    if present {
        return false;
    }
    parts.push(json!({"text": prompt}));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_are_single_segments_and_differ_by_level() {
        for level in [Level::Lite, Level::Full, Level::Ultra] {
            assert!(!caveman_prompt(level).contains("\n\n"));
            assert!(!ponytail_prompt(level).contains("\n\n"));
        }
        assert_ne!(caveman_prompt(Level::Lite), caveman_prompt(Level::Ultra));
        assert_eq!(Level::parse("full"), Some(Level::Full));
        assert_eq!(Level::parse("off"), None);
    }

    #[test]
    fn openai_injection_appends_or_inserts_and_is_idempotent() {
        let prompt = caveman_prompt(Level::Full);
        let mut body = json!({"messages": [{"role": "user", "content": "hi"}]});
        assert!(inject(&mut body, Format::OpenAi, &prompt));
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(
            !inject(&mut body, Format::OpenAi, &prompt),
            "second run is a no-op"
        );
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));

        let mut body = json!({"messages": [{"role": "system", "content": "Base."}, {"role": "user", "content": "hi"}]});
        assert!(inject(&mut body, Format::OpenAi, &prompt));
        assert_eq!(body["messages"][0]["content"], format!("Base.\n\n{prompt}"));
        let once = body.clone();
        assert!(!inject(&mut body, Format::OpenAi, &prompt));
        assert_eq!(body, once);
    }

    #[test]
    fn anthropic_injection_respects_cache_breakpoints() {
        let prompt = ponytail_prompt(Level::Lite);
        let mut body = json!({"system": [
            {"type": "text", "text": "A"},
            {"type": "text", "text": "B", "cache_control": {"type": "ephemeral"}},
            {"type": "text", "text": "C"}
        ]});
        assert!(inject(&mut body, Format::Anthropic, &prompt));
        assert_eq!(body["system"][1]["text"], prompt);
        assert_eq!(body["system"][2]["text"], "B");
        assert!(!inject(&mut body, Format::Anthropic, &prompt));
        let mut plain = json!({"system": "Base"});
        assert!(inject(&mut plain, Format::Anthropic, &prompt));
        assert!(!inject(&mut plain, Format::Anthropic, &prompt));
        let mut none = json!({"messages": []});
        assert!(inject(&mut none, Format::Anthropic, &prompt));
        assert_eq!(none["system"], prompt);
    }

    #[test]
    fn gemini_injection_pushes_a_part_once() {
        let prompt = caveman_prompt(Level::Ultra);
        let mut body = json!({"contents": []});
        assert!(inject(&mut body, Format::Gemini, &prompt));
        assert!(!inject(&mut body, Format::Gemini, &prompt));
        assert_eq!(
            body["systemInstruction"]["parts"].as_array().map(Vec::len),
            Some(1)
        );
    }
}
