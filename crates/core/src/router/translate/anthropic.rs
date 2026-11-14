// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/translator/request/openai-to-claude.js, open-sse/translator/response/claude-to-openai.js @ 1855aa4
//! OpenAI chat ⇄ Anthropic Messages.
//!
//! Requests: `system` messages become the `system` array; `user`/`tool` turns
//! become `user`, everything else `assistant`; adjacent same-role turns merge
//! (so all `tool_result`s of one step land in the user turn right after their
//! `tool_use`s); images become base64/url sources and PDFs `document`s.
//! Responses and SSE streams are mapped back to OpenAI shapes.

use super::{
    chunk, content_parts, parse_arguments, parse_data_uri, usage_chunk, StreamDecoder, Usage,
};
use crate::llm::LlmError;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Output-token cap used when the pricing table has no ceiling for a model.
pub const DEFAULT_MAX_OUTPUT: u32 = 8_192;
/// `max_tokens` sent when the request names none (capped at the ceiling).
const DEFAULT_REQUEST_MAX: u64 = 16_384;

/// Translate an OpenAI chat body into an Anthropic Messages body.
/// `ceiling` is the model's output-token limit.
#[must_use]
pub fn request(openai: &Value, model: &str, ceiling: u32) -> Value {
    let mut system: Vec<Value> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
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
                for part in content_parts(message) {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            system.push(json!({"type": "text", "text": text}));
                        }
                    }
                }
            }
            "tool" => {
                let id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let text = super::content_text(message);
                let mut block = json!({"type": "tool_result", "tool_use_id": id, "content": text});
                if message.get("is_error").and_then(Value::as_bool) == Some(true) {
                    block["is_error"] = Value::Bool(true);
                }
                push_merged(&mut messages, "user", vec![block]);
            }
            "user" => push_merged(&mut messages, "user", user_blocks(message)),
            _ => push_merged(&mut messages, "assistant", assistant_blocks(message)),
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    let requested = openai
        .get("max_completion_tokens")
        .or_else(|| openai.get("max_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_REQUEST_MAX);
    body.insert(
        "max_tokens".into(),
        json!(requested.min(u64::from(ceiling))),
    );
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    body.insert("messages".into(), Value::Array(messages));
    if let Some(tools) = openai.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools.iter().filter_map(tool).collect();
        if !converted.is_empty() {
            body.insert("tools".into(), Value::Array(converted));
            if let Some(choice) = openai.get("tool_choice") {
                body.insert("tool_choice".into(), tool_choice(choice));
            }
        }
    }
    for (from, to) in [("temperature", "temperature"), ("top_p", "top_p")] {
        if let Some(value) = openai.get(from).filter(|v| v.is_number()) {
            body.insert(to.into(), value.clone());
        }
    }
    match openai.get("stop") {
        Some(Value::String(stop)) => {
            body.insert("stop_sequences".into(), json!([stop]));
        }
        Some(Value::Array(stops)) if !stops.is_empty() => {
            body.insert("stop_sequences".into(), Value::Array(stops.clone()));
        }
        _ => {}
    }
    if openai.get("stream").and_then(Value::as_bool) == Some(true) {
        body.insert("stream".into(), Value::Bool(true));
    }
    Value::Object(body)
}

fn push_merged(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut() {
        if last["role"] == role {
            if let Some(content) = last["content"].as_array_mut() {
                content.extend(blocks);
                return;
            }
        }
    }
    messages.push(json!({"role": role, "content": blocks}));
}

fn user_blocks(message: &Value) -> Vec<Value> {
    content_parts(message)
        .iter()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => part
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(|t| json!({"type": "text", "text": t})),
            Some("image_url") => {
                let url = part
                    .pointer("/image_url/url")
                    .or_else(|| part.get("image_url"))
                    .and_then(Value::as_str)?;
                Some(media_block(url))
            }
            Some("file") => {
                let data = part.pointer("/file/file_data").and_then(Value::as_str)?;
                Some(media_block(data))
            }
            _ => None,
        })
        .collect()
}

/// Image or PDF block for a data URI or http(s) URL.
fn media_block(url: &str) -> Value {
    if let Some((mime, data)) = parse_data_uri(url) {
        let kind = if mime == "application/pdf" {
            "document"
        } else {
            "image"
        };
        return json!({"type": kind, "source": {"type": "base64", "media_type": mime, "data": data}});
    }
    let kind = if super::guess_mime(url) == "application/pdf" {
        "document"
    } else {
        "image"
    };
    json!({"type": kind, "source": {"type": "url", "url": url}})
}

/// Anthropic thinking blocks from OpenRouter-style `reasoning_details`
/// (they must come back unchanged, signature included, during tool use).
fn thinking_blocks(message: &Value) -> Vec<Value> {
    let details = message.get("reasoning_details").and_then(Value::as_array);
    details
        .into_iter()
        .flatten()
        .filter(|d| d.get("format").and_then(Value::as_str) == Some(REASONING_FORMAT))
        .filter_map(|d| match d.get("type").and_then(Value::as_str) {
            Some("reasoning.text") => Some(json!({
                "type": "thinking",
                "thinking": d.get("text").and_then(Value::as_str).unwrap_or(""),
                "signature": d.get("signature").and_then(Value::as_str).unwrap_or(""),
            })),
            Some("reasoning.encrypted") => Some(json!({
                "type": "redacted_thinking",
                "data": d.get("data").and_then(Value::as_str).unwrap_or(""),
            })),
            _ => None,
        })
        .collect()
}

fn assistant_blocks(message: &Value) -> Vec<Value> {
    let mut blocks = thinking_blocks(message);
    blocks.extend(
        content_parts(message)
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|t| !t.is_empty())
            .map(|t| json!({"type": "text", "text": t})),
    );
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let arguments = call
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        blocks.push(json!({
            "type": "tool_use",
            "id": call.get("id").and_then(Value::as_str).unwrap_or_default(),
            "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or_default(),
            "input": parse_arguments(arguments),
        }));
    }
    blocks
}

fn tool(tool: &Value) -> Option<Value> {
    let function = tool.get("function")?;
    let mut out = json!({
        "name": function.get("name")?.as_str()?,
        "input_schema": function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
    });
    if let Some(description) = function.get("description").and_then(Value::as_str) {
        out["description"] = json!(description);
    }
    Some(out)
}

fn tool_choice(choice: &Value) -> Value {
    match choice {
        Value::String(s) if s == "required" => json!({"type": "any"}),
        Value::Object(_) => match choice.pointer("/function/name").and_then(Value::as_str) {
            Some(name) => json!({"type": "tool", "name": name}),
            None => json!({"type": "auto"}),
        },
        _ => json!({"type": "auto"}),
    }
}

/// Anthropic `stop_reason` → OpenAI `finish_reason`.
#[must_use]
pub fn finish_reason(stop: &str) -> &'static str {
    match stop {
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

fn usage(raw: &Value) -> Usage {
    let int = |key: &str| raw.get(key).and_then(Value::as_i64).unwrap_or(0);
    let cached = int("cache_read_input_tokens");
    let creation = int("cache_creation_input_tokens");
    Usage {
        prompt: int("input_tokens") + cached + creation,
        completion: int("output_tokens"),
        cached,
        cache_creation: creation,
        reasoning: 0,
    }
}

/// Translate a whole (non-streamed) Anthropic message into a `chat.completion`.
#[must_use]
pub fn response(message: &Value) -> Value {
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut details = Vec::new();
    for block in message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("thinking") => details.push(json!({
                "type": "reasoning.text", "format": REASONING_FORMAT, "index": details.len(),
                "text": block.get("thinking").cloned().unwrap_or(Value::Null),
                "signature": block.get("signature").cloned().unwrap_or(Value::Null),
            })),
            Some("redacted_thinking") => details.push(json!({
                "type": "reasoning.encrypted", "format": REASONING_FORMAT, "index": details.len(),
                "data": block.get("data").cloned().unwrap_or(Value::Null),
            })),
            Some("tool_use") => calls.push(json!({
                "id": block.get("id").cloned().unwrap_or(Value::Null),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or(Value::Null),
                    "arguments": block.get("input").map_or_else(|| "{}".into(), Value::to_string),
                },
            })),
            _ => {}
        }
    }
    let mut out_message = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
    if !calls.is_empty() {
        out_message["tool_calls"] = Value::Array(calls);
    }
    if !details.is_empty() {
        out_message["reasoning_details"] = Value::Array(details);
    }
    let stop = message
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    json!({
        "id": message.get("id").cloned().unwrap_or(Value::Null),
        "object": "chat.completion",
        "model": message.get("model").cloned().unwrap_or(Value::Null),
        "choices": [{"index": 0, "message": out_message, "finish_reason": finish_reason(stop)}],
        "usage": message.get("usage").map_or_else(Usage::default, usage).to_openai(),
    })
}

/// `reasoning_details` format tag for Anthropic thinking blocks.
pub const REASONING_FORMAT: &str = "anthropic-claude-v1";

fn detail(mut entry: Value) -> Value {
    entry["format"] = json!(REASONING_FORMAT);
    chunk(json!({"reasoning_details": [entry]}), None)
}

/// Anthropic SSE (`message_start`, `content_block_*`, `message_delta`, …) →
/// OpenAI chunks.
#[derive(Debug, Default)]
pub struct AnthropicDecoder {
    /// Content-block index → OpenAI tool-call index.
    tools: HashMap<u64, u64>,
    usage: Usage,
}

impl StreamDecoder for AnthropicDecoder {
    fn decode(&mut self, _event: &str, data: &Value) -> Result<Vec<Value>, LlmError> {
        let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
        let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
        Ok(match kind {
            "message_start" => {
                if let Some(raw) = data.pointer("/message/usage") {
                    self.usage = usage(raw);
                }
                Vec::new()
            }
            "content_block_start" => {
                let block = &data["content_block"];
                match block.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        let next = self.tools.len() as u64;
                        let tool_index = *self.tools.entry(index).or_insert(next);
                        vec![chunk(
                            json!({"tool_calls": [{
                                "index": tool_index,
                                "id": block.get("id").cloned().unwrap_or(Value::Null),
                                "type": "function",
                                "function": {"name": block.get("name").cloned().unwrap_or(Value::Null), "arguments": ""},
                            }]}),
                            None,
                        )]
                    }
                    Some("text") => match block.get("text").and_then(Value::as_str) {
                        Some(text) if !text.is_empty() => {
                            vec![chunk(json!({"content": text}), None)]
                        }
                        _ => Vec::new(),
                    },
                    Some("thinking") => vec![detail(json!({
                        "type": "reasoning.text", "index": index,
                        "text": block.get("thinking").and_then(Value::as_str).unwrap_or(""),
                    }))],
                    Some("redacted_thinking") => vec![detail(json!({
                        "type": "reasoning.encrypted", "index": index,
                        "data": block.get("data").and_then(Value::as_str).unwrap_or(""),
                    }))],
                    _ => Vec::new(),
                }
            }
            "content_block_delta" => {
                let delta = &data["delta"];
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        vec![chunk(
                            json!({"content": delta["text"].as_str().unwrap_or("")}),
                            None,
                        )]
                    }
                    Some("thinking_delta") => vec![detail(json!({
                        "type": "reasoning.text", "index": index,
                        "text": delta["thinking"].as_str().unwrap_or(""),
                    }))],
                    Some("signature_delta") => vec![detail(json!({
                        "type": "reasoning.text", "index": index,
                        "signature": delta["signature"].as_str().unwrap_or(""),
                    }))],
                    Some("input_json_delta") => match self.tools.get(&index) {
                        Some(tool_index) => vec![chunk(
                            json!({"tool_calls": [{
                                "index": tool_index,
                                "function": {"arguments": delta["partial_json"].as_str().unwrap_or("")},
                            }]}),
                            None,
                        )],
                        None => Vec::new(),
                    },
                    _ => Vec::new(),
                }
            }
            "message_delta" => {
                if let Some(output) = data.pointer("/usage/output_tokens").and_then(Value::as_i64) {
                    self.usage.completion = output;
                }
                match data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    Some(stop) => vec![chunk(json!({}), Some(finish_reason(stop)))],
                    None => Vec::new(),
                }
            }
            "message_stop" => vec![usage_chunk(&self.usage)],
            "error" => {
                let message = data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(LlmError::Protocol(format!(
                    "anthropic stream error: {message}"
                )));
            }
            _ => Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::StreamAccumulator;

    #[test]
    fn golden_request_translation() {
        let openai = json!({
            "model": "anthropic/claude-x",
            "stream": true,
            "max_tokens": 999_999,
            "temperature": 0.2,
            "stop": "END",
            "tool_choice": "required",
            "messages": [
                {"role": "system", "content": "Be brief."},
                {"role": "developer", "content": [{"type": "text", "text": "Use tools."}]},
                {"role": "user", "content": [
                    {"type": "text", "text": "What is in these?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}},
                    {"type": "image_url", "image_url": {"url": "https://example.com/cat.jpg"}},
                    {"type": "file", "file": {"file_data": "data:application/pdf;base64,JVBER", "filename": "a.pdf"}}
                ]},
                {"role": "assistant", "content": "Let me look.", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a\"}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "list_dir", "arguments": ""}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "file a"},
                {"role": "tool", "tool_call_id": "call_2", "content": "a b"},
                {"role": "user", "content": "thanks"}
            ],
            "tools": [{"type": "function", "function": {"name": "read_file", "description": "Read", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}}]
        });
        let expected = json!({
            "model": "claude-x",
            "max_tokens": 64000,
            "system": [{"type": "text", "text": "Be brief."}, {"type": "text", "text": "Use tools."}],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "What is in these?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBOR"}},
                    {"type": "image", "source": {"type": "url", "url": "https://example.com/cat.jpg"}},
                    {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBER"}}
                ]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me look."},
                    {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "a"}},
                    {"type": "tool_use", "id": "call_2", "name": "list_dir", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "file a"},
                    {"type": "tool_result", "tool_use_id": "call_2", "content": "a b"},
                    {"type": "text", "text": "thanks"}
                ]}
            ],
            "tools": [{"name": "read_file", "description": "Read", "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}}],
            "tool_choice": {"type": "any"},
            "temperature": 0.2,
            "stop_sequences": ["END"],
            "stream": true
        });
        assert_eq!(request(&openai, "claude-x", 64_000), expected);
    }

    #[test]
    fn tool_choice_and_default_max_tokens() {
        assert_eq!(
            tool_choice(&json!({"type":"function","function":{"name":"x"}})),
            json!({"type":"tool","name":"x"})
        );
        assert_eq!(tool_choice(&json!("auto")), json!({"type":"auto"}));
        assert_eq!(tool_choice(&json!("none")), json!({"type":"auto"}));
        let body = request(
            &json!({"messages":[{"role":"user","content":"hi"}]}),
            "m",
            4_096,
        );
        assert_eq!(body["max_tokens"], 4096);
        assert!(body.get("system").is_none() && body.get("tools").is_none());
    }

    #[test]
    fn golden_response_translation() {
        let message = json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
            "content": [
                {"type": "thinking", "thinking": "hmm"},
                {"type": "text", "text": "Reading."},
                {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "a"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20}
        });
        let out = response(&message);
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["choices"][0]["message"]["content"], "Reading.");
        assert_eq!(
            out["choices"][0]["message"]["tool_calls"][0],
            json!({"id":"toolu_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}})
        );
        let usage = Usage::from_openai(&out["usage"]).expect("usage");
        assert_eq!(
            usage,
            Usage {
                prompt: 130,
                completion: 5,
                cached: 100,
                cache_creation: 20,
                reasoning: 0
            }
        );
    }

    #[test]
    fn stream_decodes_text_tools_finish_and_usage() {
        let events = [
            json!({"type":"message_start","message":{"usage":{"input_tokens":7,"output_tokens":1,"cache_read_input_tokens":3}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"ping"}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi "}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"there"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_9","name":"list_dir","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"pa"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"th\":\".\"}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":12}}),
            json!({"type":"message_stop"}),
        ];
        let mut decoder = AnthropicDecoder::default();
        let mut acc = StreamAccumulator::new();
        let mut usage = None;
        for event in &events {
            for chunk in decoder.decode("", event).expect("decode") {
                if let Some(u) = chunk.get("usage") {
                    usage = Usage::from_openai(u);
                }
                acc.push(&chunk).expect("push");
            }
        }
        let done = acc.finish();
        assert_eq!(done.content, "Hi there");
        assert_eq!(done.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(done.tool_calls[0].id, "toolu_9");
        assert_eq!(done.tool_calls[0].function.arguments, "{\"path\":\".\"}");
        assert_eq!(usage.expect("usage").prompt, 10);
        assert_eq!(usage.expect("usage").completion, 12);
        assert!(decoder
            .decode(
                "error",
                &json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}})
            )
            .is_err());
    }
    #[test]
    fn thinking_round_trips_through_reasoning_details() {
        let mut decoder = AnthropicDecoder::default();
        let mut deltas = Vec::new();
        for event in [
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Plan: read"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "SIG"}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "redacted_thinking", "data": "ENC"}}),
        ] {
            for piece in decoder.decode("", &event).expect("decode") {
                if let Some(found) = piece
                    .pointer("/choices/0/delta/reasoning_details")
                    .and_then(Value::as_array)
                {
                    crate::router::reasoning::merge_details(&mut deltas, found);
                }
            }
        }
        assert_eq!(deltas[0]["text"], "Plan: read");
        assert_eq!(deltas[0]["signature"], "SIG");
        assert_eq!(deltas[1]["data"], "ENC");
        let openai = json!({"messages": [
            {"role": "user", "content": "go"},
            {"role": "assistant", "content": null, "reasoning_details": deltas,
             "tool_calls": [{"id": "t1", "type": "function", "function": {"name": "read", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "t1", "content": "ok"}
        ]});
        let body = request(&openai, "claude", 8192);
        let blocks = &body["messages"][1]["content"];
        assert_eq!(
            blocks[0],
            json!({"type": "thinking", "thinking": "Plan: read", "signature": "SIG"})
        );
        assert_eq!(
            blocks[1],
            json!({"type": "redacted_thinking", "data": "ENC"})
        );
        assert_eq!(blocks[2]["type"], "tool_use");
        let whole = response(
            &json!({"content": [{"type": "thinking", "thinking": "hm", "signature": "S"}, {"type": "text", "text": "hi"}]}),
        );
        assert_eq!(
            whole["choices"][0]["message"]["reasoning_details"][0]["signature"],
            "S"
        );
    }
}
