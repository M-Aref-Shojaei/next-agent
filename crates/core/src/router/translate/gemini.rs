// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/translator/request/openai-to-gemini.js, open-sse/translator/response/gemini-to-openai.js @ 1855aa4
//! OpenAI chat ⇄ Gemini `generateContent`.
//!
//! Messages become `contents` (roles `user`/`model`), tool calls
//! `functionCall`, tool results `functionResponse`; the system prompt goes to
//! `systemInstruction` and `max_tokens` to `generationConfig.maxOutputTokens`.
//! Function names are sanitised to Gemini's rules (≤ 64 chars) and mapped back
//! on the way out; duplicate call ids get a `-2`, `-3`… suffix.

use super::{
    chunk, content_parts, parse_arguments, parse_data_uri, usage_chunk, StreamDecoder, Usage,
};
use crate::llm::LlmError;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// Sanitised function name → the name the agent used.
pub type NameMap = HashMap<String, String>;

/// Gemini function names: `[A-Za-z_][A-Za-z0-9_.-]{0,63}`.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        out.insert(0, '_');
    }
    out.truncate(64);
    out
}

/// JSON-schema keys Gemini's `Schema` accepts; everything else is dropped.
const SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "properties",
    "required",
    "items",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "anyOf",
    "title",
    "minLength",
    "maxLength",
    "pattern",
];

fn clean_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                if !SCHEMA_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let cleaned = match key.as_str() {
                    "properties" => Value::Object(
                        value
                            .as_object()
                            .into_iter()
                            .flatten()
                            .map(|(k, v)| (k.clone(), clean_schema(v)))
                            .collect(),
                    ),
                    "items" => clean_schema(value),
                    "anyOf" => Value::Array(
                        value
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(clean_schema)
                            .collect(),
                    ),
                    _ => value.clone(),
                };
                out.insert(key.clone(), cleaned);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Translate an OpenAI chat body; returns the Gemini body and the name map
/// needed to decode the response.
#[must_use]
#[allow(clippy::too_many_lines)] // One pass over messages, tools and options.
pub fn request(openai: &Value) -> (Value, NameMap) {
    let mut names = NameMap::new();
    let mut call_names: HashMap<String, String> = HashMap::new();
    let mut system: Vec<Value> = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    for message in openai
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match role {
            "system" | "developer" => {
                let text = super::content_text(message);
                if !text.is_empty() {
                    system.push(json!({"text": text}));
                }
            }
            "user" => push_merged(&mut contents, "user", user_parts(message)),
            "tool" => {
                let id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let name = call_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| sanitize_name(id));
                let part = json!({"functionResponse": {
                    "name": name,
                    "response": {"content": super::content_text(message)},
                }});
                push_merged(&mut contents, "user", vec![part]);
            }
            _ => {
                let mut parts: Vec<Value> = content_parts(message)
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .filter(|t| !t.is_empty())
                    .map(|t| json!({"text": t}))
                    .collect();
                for call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let original = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let name = sanitize_name(original);
                    names.insert(name.clone(), original.to_string());
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        call_names.insert(id.to_string(), name.clone());
                    }
                    let args = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    parts.push(
                        json!({"functionCall": {"name": name, "args": parse_arguments(args)}}),
                    );
                }
                push_merged(&mut contents, "model", parts);
            }
        }
    }
    let mut body = Map::new();
    body.insert("contents".into(), Value::Array(contents));
    if !system.is_empty() {
        body.insert("systemInstruction".into(), json!({"parts": system}));
    }
    let declarations: Vec<Value> = openai
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let function = tool.get("function")?;
            let original = function.get("name")?.as_str()?;
            let name = sanitize_name(original);
            names.insert(name.clone(), original.to_string());
            let mut out = json!({"name": name});
            if let Some(d) = function.get("description") {
                out["description"] = d.clone();
            }
            if let Some(p) = function.get("parameters") {
                out["parameters"] = clean_schema(p);
            }
            Some(out)
        })
        .collect();
    if !declarations.is_empty() {
        body.insert(
            "tools".into(),
            json!([{"functionDeclarations": declarations}]),
        );
        if let Some(choice) = openai.get("tool_choice") {
            let config = match choice {
                Value::String(s) if s == "required" => json!({"mode": "ANY"}),
                Value::String(s) if s == "none" => json!({"mode": "NONE"}),
                Value::Object(_) => {
                    match choice.pointer("/function/name").and_then(Value::as_str) {
                        Some(name) => {
                            json!({"mode": "ANY", "allowedFunctionNames": [sanitize_name(name)]})
                        }
                        None => json!({"mode": "AUTO"}),
                    }
                }
                _ => json!({"mode": "AUTO"}),
            };
            body.insert(
                "toolConfig".into(),
                json!({"functionCallingConfig": config}),
            );
        }
    }
    let mut generation = Map::new();
    if let Some(max) = openai
        .get("max_completion_tokens")
        .or_else(|| openai.get("max_tokens"))
        .filter(|v| v.is_u64())
    {
        generation.insert("maxOutputTokens".into(), max.clone());
    }
    for (from, to) in [("temperature", "temperature"), ("top_p", "topP")] {
        if let Some(v) = openai.get(from).filter(|v| v.is_number()) {
            generation.insert(to.into(), v.clone());
        }
    }
    match openai.get("stop") {
        Some(Value::String(s)) => {
            generation.insert("stopSequences".into(), json!([s]));
        }
        Some(Value::Array(a)) if !a.is_empty() => {
            generation.insert("stopSequences".into(), Value::Array(a.clone()));
        }
        _ => {}
    }
    if !generation.is_empty() {
        body.insert("generationConfig".into(), Value::Object(generation));
    }
    (Value::Object(body), names)
}

fn push_merged(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = contents.last_mut() {
        if last["role"] == role {
            if let Some(existing) = last["parts"].as_array_mut() {
                existing.extend(parts);
                return;
            }
        }
    }
    contents.push(json!({"role": role, "parts": parts}));
}

fn user_parts(message: &Value) -> Vec<Value> {
    content_parts(message)
        .iter()
        .filter_map(|part| {
            let url = match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    return part
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                        .map(|t| json!({"text": t}))
                }
                Some("image_url") => part
                    .pointer("/image_url/url")
                    .or_else(|| part.get("image_url"))
                    .and_then(Value::as_str)?,
                Some("file") => part.pointer("/file/file_data").and_then(Value::as_str)?,
                _ => return None,
            };
            Some(match parse_data_uri(url) {
                Some((mime, data)) => json!({"inlineData": {"mimeType": mime, "data": data}}),
                None => json!({"fileData": {"mimeType": super::guess_mime(url), "fileUri": url}}),
            })
        })
        .collect()
}

/// Gemini `finishReason` → OpenAI `finish_reason`.
#[must_use]
pub fn finish_reason(reason: &str, had_tool_calls: bool) -> &'static str {
    match reason {
        "MAX_TOKENS" => "length",
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => "content_filter",
        _ if had_tool_calls => "tool_calls",
        _ => "stop",
    }
}

fn usage(meta: &Value) -> Usage {
    let int = |key: &str| meta.get(key).and_then(Value::as_i64).unwrap_or(0);
    let thoughts = int("thoughtsTokenCount");
    Usage {
        prompt: int("promptTokenCount"),
        completion: int("candidatesTokenCount") + thoughts,
        cached: int("cachedContentTokenCount"),
        cache_creation: 0,
        reasoning: thoughts,
    }
}

/// Stream decoder; also used (one event) for whole responses.
#[derive(Debug, Default)]
pub struct GeminiDecoder {
    names: NameMap,
    calls: u64,
    ids: HashSet<String>,
    usage: Usage,
}

impl GeminiDecoder {
    #[must_use]
    pub fn new(names: NameMap) -> Self {
        Self {
            names,
            ..Self::default()
        }
    }

    fn unique_id(&mut self, wanted: &str) -> String {
        let mut id = wanted.to_string();
        let mut n = 2;
        while !self.ids.insert(id.clone()) {
            id = format!("{wanted}-{n}");
            n += 1;
        }
        id
    }
}

impl StreamDecoder for GeminiDecoder {
    fn decode(&mut self, _event: &str, data: &Value) -> Result<Vec<Value>, LlmError> {
        if let Some(error) = data.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(LlmError::Protocol(format!(
                "gemini stream error: {message}"
            )));
        }
        let mut out = Vec::new();
        let candidate = data.pointer("/candidates/0");
        for part in candidate
            .and_then(|c| c.pointer("/content/parts"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    out.push(chunk(json!({"content": text}), None));
                }
            }
            if let Some(call) = part.get("functionCall") {
                let sent = call.get("name").and_then(Value::as_str).unwrap_or("");
                let name = self
                    .names
                    .get(sent)
                    .cloned()
                    .unwrap_or_else(|| sent.to_string());
                let wanted = call
                    .get("id")
                    .and_then(Value::as_str)
                    .map_or_else(|| format!("call_{}", self.calls), str::to_string);
                let id = self.unique_id(&wanted);
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                out.push(chunk(
                    json!({"tool_calls": [{
                        "index": self.calls, "id": id, "type": "function",
                        "function": {"name": name, "arguments": args.to_string()},
                    }]}),
                    None,
                ));
                self.calls += 1;
            }
        }
        if let Some(meta) = data.get("usageMetadata") {
            self.usage = usage(meta);
        }
        if let Some(reason) = candidate
            .and_then(|c| c.get("finishReason"))
            .and_then(Value::as_str)
        {
            out.push(chunk(
                json!({}),
                Some(finish_reason(reason, self.calls > 0)),
            ));
            out.push(usage_chunk(&self.usage));
        }
        Ok(out)
    }
}

/// Translate a whole Gemini response into a `chat.completion`.
#[must_use]
pub fn response(body: &Value, names: &NameMap) -> Value {
    let mut decoder = GeminiDecoder::new(names.clone());
    let mut acc = crate::llm::StreamAccumulator::new();
    let mut usage_value = Usage::default().to_openai();
    for piece in decoder.decode("", body).unwrap_or_default() {
        if let Some(u) = piece.get("usage") {
            usage_value = u.clone();
        }
        let _ = acc.push(&piece);
    }
    let done = acc.finish();
    let mut message = json!({"role": "assistant", "content": if done.content.is_empty() { Value::Null } else { json!(done.content) }});
    if !done.tool_calls.is_empty() {
        message["tool_calls"] = serde_json::to_value(&done.tool_calls).unwrap_or_default();
    }
    json!({
        "object": "chat.completion",
        "choices": [{"index": 0, "message": message, "finish_reason": done.finish_reason.unwrap_or_else(|| "stop".into())}],
        "usage": usage_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_request_translation() {
        let openai = json!({
            "max_tokens": 500,
            "top_p": 0.9,
            "tool_choice": {"type": "function", "function": {"name": "read file"}},
            "messages": [
                {"role": "system", "content": "Be brief."},
                {"role": "user", "content": [
                    {"type": "text", "text": "Look"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}},
                    {"type": "image_url", "image_url": {"url": "https://example.com/doc.pdf"}}
                ]},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "read file", "arguments": "{\"path\":\"a\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "contents"},
                {"role": "user", "content": "ok"}
            ],
            "tools": [{"type": "function", "function": {
                "name": "read file", "description": "Read",
                "parameters": {"type": "object", "additionalProperties": false, "$schema": "x",
                    "properties": {"path": {"type": "string", "default": "."}}, "required": ["path"]}
            }}]
        });
        let (body, names) = request(&openai);
        let expected = json!({
            "contents": [
                {"role": "user", "parts": [
                    {"text": "Look"},
                    {"inlineData": {"mimeType": "image/png", "data": "iVBOR"}},
                    {"fileData": {"mimeType": "application/pdf", "fileUri": "https://example.com/doc.pdf"}}
                ]},
                {"role": "model", "parts": [{"functionCall": {"name": "read_file", "args": {"path": "a"}}}]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "read_file", "response": {"content": "contents"}}},
                    {"text": "ok"}
                ]}
            ],
            "systemInstruction": {"parts": [{"text": "Be brief."}]},
            "tools": [{"functionDeclarations": [{"name": "read_file", "description": "Read",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}]}],
            "toolConfig": {"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["read_file"]}},
            "generationConfig": {"maxOutputTokens": 500, "topP": 0.9}
        });
        assert_eq!(body, expected);
        assert_eq!(
            names.get("read_file").map(String::as_str),
            Some("read file")
        );
    }

    #[test]
    fn names_are_sanitised() {
        assert_eq!(sanitize_name("mcp:server/tool"), "mcp_server_tool");
        assert_eq!(sanitize_name("9lives"), "_9lives");
        assert_eq!(sanitize_name(&"x".repeat(80)).len(), 64);
    }

    #[test]
    fn golden_response_with_duplicate_ids_and_usage() {
        let mut names = NameMap::new();
        names.insert("read_file".into(), "read file".into());
        let body = json!({
            "candidates": [{"content": {"role": "model", "parts": [
                {"text": "thinking...", "thought": true},
                {"text": "Two reads."},
                {"functionCall": {"id": "x", "name": "read_file", "args": {"path": "a"}}},
                {"functionCall": {"id": "x", "name": "read_file", "args": {"path": "b"}}}
            ]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 10, "thoughtsTokenCount": 4, "cachedContentTokenCount": 20}
        });
        let out = response(&body, &names);
        let message = &out["choices"][0]["message"];
        assert_eq!(message["content"], "Two reads.");
        assert_eq!(message["tool_calls"][0]["id"], "x");
        assert_eq!(message["tool_calls"][1]["id"], "x-2");
        assert_eq!(message["tool_calls"][1]["function"]["name"], "read file");
        assert_eq!(
            message["tool_calls"][1]["function"]["arguments"],
            "{\"path\":\"b\"}"
        );
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        let usage = Usage::from_openai(&out["usage"]).expect("usage");
        assert_eq!(
            usage,
            Usage {
                prompt: 50,
                completion: 14,
                cached: 20,
                cache_creation: 0,
                reasoning: 4
            }
        );
    }

    #[test]
    fn stream_errors_and_finish_reasons() {
        let mut decoder = GeminiDecoder::default();
        assert!(decoder
            .decode("", &json!({"error": {"message": "quota"}}))
            .is_err());
        assert_eq!(finish_reason("MAX_TOKENS", false), "length");
        assert_eq!(finish_reason("SAFETY", true), "content_filter");
        assert_eq!(finish_reason("STOP", false), "stop");
    }
}
