//! LLM transport seam (`CompletionClient` in Module Boundaries).
//!
//! OpenAI-compatible chat types plus the [`CompletionClient`] trait the
//! [`BuiltinExecutor`](crate::builtin::BuiltinExecutor) depends on. No HTTP
//! here: the router adapter (`crate::router`) implements the trait, and tests
//! substitute scripted clients.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One chat message in `OpenAI` wire format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    #[must_use]
    pub fn system(text: impl Into<String>) -> Self {
        Self::text("system", text)
    }

    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self::text("user", text)
    }

    #[must_use]
    pub fn assistant(text: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: text,
            tool_calls,
            tool_call_id: None,
        }
    }

    #[must_use]
    pub fn tool(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(output.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }

    fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(text.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// A function call requested by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_type() -> String {
    "function".into()
}

/// Name + raw JSON arguments, exactly as the model produced them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A chat completion request. `stream` is set by the transport.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
}

/// Aggregated result of one (streamed) completion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
}

/// Transport failures, mapped to clear UI errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// Bad configuration (e.g. malformed base URL).
    Config(String),
    /// 401/403: the API key is missing or wrong.
    Unauthorized,
    /// 404 or an explicit "model not found".
    ModelNotFound(String),
    /// Connection refused, DNS failure or timeout.
    Unavailable(String),
    /// Any other non-success HTTP status (after retries for 429/5xx).
    Http { status: u16, body: String },
    /// The stream or body did not match the `OpenAI` format.
    Protocol(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(f, "router configuration error: {message}"),
            Self::Unauthorized => {
                f.write_str("router rejected the API key (401); check Settings → Router")
            }
            Self::ModelNotFound(model) => {
                write!(f, "model `{model}` is not available on the router")
            }
            Self::Unavailable(message) => write!(f, "router unreachable: {message}"),
            Self::Http { status, body } => write!(f, "router returned HTTP {status}: {body}"),
            Self::Protocol(message) => write!(f, "unexpected router response: {message}"),
        }
    }
}

impl std::error::Error for LlmError {}

/// Blocking chat-completions client. Called from the task thread; streaming
/// text arrives through `on_delta` while the call is in flight.
pub trait CompletionClient: Send + Sync {
    /// Run one completion.
    ///
    /// # Errors
    /// Returns [`LlmError`] for transport, auth or protocol failures.
    fn complete(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError>;
}

/// Accumulates streamed `choices[0].delta` chunks into a [`Completion`].
///
/// Tool calls stream as fragments keyed by `index`: the first fragment carries
/// `id` and `function.name`, later ones append to `function.arguments`.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    content: String,
    calls: BTreeMap<u64, ToolCall>,
    finish_reason: Option<String>,
}

impl StreamAccumulator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one parsed SSE `data:` payload. Returns the text delta, if any.
    ///
    /// # Errors
    /// Returns [`LlmError::Protocol`] if the chunk is not a completion chunk.
    pub fn push(&mut self, chunk: &serde_json::Value) -> Result<Option<String>, LlmError> {
        if let Some(error) = chunk.get("error") {
            return Err(LlmError::Protocol(format!("stream error: {error}")));
        }
        let Some(choice) = chunk
            .get("choices")
            .and_then(|choices| choices.as_array())
            .and_then(|choices| choices.first())
        else {
            // Usage-only or keep-alive chunks carry no choices.
            return Ok(None);
        };
        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
            self.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta") else {
            return Ok(None);
        };
        if let Some(calls) = delta.get("tool_calls").and_then(|calls| calls.as_array()) {
            for (position, call) in calls.iter().enumerate() {
                let index = call
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(position as u64);
                let entry = self.calls.entry(index).or_insert_with(|| ToolCall {
                    id: String::new(),
                    kind: function_type(),
                    function: FunctionCall {
                        name: String::new(),
                        arguments: String::new(),
                    },
                });
                if let Some(id) = call.get("id").and_then(|id| id.as_str()) {
                    id.clone_into(&mut entry.id);
                }
                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                        entry.function.name.push_str(name);
                    }
                    if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                        entry.function.arguments.push_str(args);
                    }
                }
            }
        }
        match delta.get("content").and_then(|c| c.as_str()) {
            Some(text) if !text.is_empty() => {
                self.content.push_str(text);
                Ok(Some(text.to_string()))
            }
            _ => Ok(None),
        }
    }

    /// Finish, assigning ids to calls the server left anonymous.
    #[must_use]
    pub fn finish(self) -> Completion {
        let tool_calls = self
            .calls
            .into_iter()
            .map(|(index, mut call)| {
                if call.id.is_empty() {
                    call.id = format!("call_{index}");
                }
                call
            })
            .collect();
        Completion {
            content: self.content,
            tool_calls,
            finish_reason: self.finish_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accumulates_text_and_fragmented_tool_calls() {
        let mut acc = StreamAccumulator::new();
        let chunks = [
            json!({"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}),
            json!({"choices":[{"delta":{"content":"lo"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"name":"list_dir","arguments":"{}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            json!({"usage":{"total_tokens":3}}),
        ];
        let deltas: Vec<String> = chunks
            .iter()
            .filter_map(|chunk| acc.push(chunk).expect("chunk"))
            .collect();
        assert_eq!(deltas, ["Hel", "lo"]);
        let done = acc.finish();
        assert_eq!(done.content, "Hello");
        assert_eq!(done.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(done.tool_calls.len(), 2);
        assert_eq!(done.tool_calls[0].id, "call_a");
        assert_eq!(done.tool_calls[0].function.arguments, "{\"path\":\"a\"}");
        assert_eq!(done.tool_calls[1].id, "call_1");
        assert_eq!(done.tool_calls[1].function.name, "list_dir");
    }

    #[test]
    fn stream_errors_surface() {
        let mut acc = StreamAccumulator::new();
        assert!(acc.push(&json!({"error":{"message":"boom"}})).is_err());
    }

    #[test]
    fn messages_serialize_in_openai_shape() {
        let call = ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        };
        let value = serde_json::to_value(ChatMessage::assistant(None, vec![call])).expect("json");
        assert_eq!(value["tool_calls"][0]["type"], "function");
        assert!(value.get("content").is_none());
        let tool = serde_json::to_value(ChatMessage::tool("c1", "out")).expect("json");
        assert_eq!(
            tool,
            json!({"role":"tool","content":"out","tool_call_id":"c1"})
        );
    }
}
