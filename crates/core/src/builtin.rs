//! `BuiltinExecutor`: a minimal OpenAI-compatible tool-calling loop
//! (ADR-0006). Talks to the LLM only through [`CompletionClient`] and performs
//! side effects only by returning [`AgentStep::Call`] to the runner/broker.
//!
//! Loop: send messages + tool schemas → stream `message_delta` → for each
//! tool call yield a `Call` → append the result as a `tool` message → repeat
//! until the model stops calling tools or the step budget is spent.

use crate::agent::{AgentExecutor, AgentStep, TaskSpec};
use crate::events::{Emitter, EventKind};
use crate::llm::{ChatMessage, ChatRequest, CompletionClient, ToolCall};
use crate::tools::{ToolRequest, ToolResult};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default number of model turns per task (Agent Execution: 50).
pub const DEFAULT_STEP_BUDGET: usize = 50;

/// Longest tool output fed back to the model.
pub const TOOL_RESULT_LIMIT: usize = 16 * 1024;

enum Queued {
    Emit(EventKind),
    Call {
        call_id: String,
        request: ToolRequest,
    },
    /// The model asked for something unparseable; answer it without a call.
    Invalid {
        call_id: String,
        error: String,
    },
}

/// Built-in agent loop over an OpenAI-compatible router.
pub struct BuiltinExecutor {
    client: Arc<dyn CompletionClient>,
    root: PathBuf,
    budget: usize,
    model: String,
    plan_mode: bool,
    messages: Vec<ChatMessage>,
    queue: VecDeque<Queued>,
    turns: usize,
    finished: bool,
    last_text: String,
    events: Option<Emitter>,
}

impl std::fmt::Debug for BuiltinExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltinExecutor")
            .field("root", &self.root)
            .field("model", &self.model)
            .field("turns", &self.turns)
            .field("messages", &self.messages.len())
            .finish_non_exhaustive()
    }
}

impl BuiltinExecutor {
    /// `workspace_root` resolves relative paths the model uses (the sandbox
    /// root, i.e. the worktree path in `auto` mode).
    #[must_use]
    pub fn new(client: Arc<dyn CompletionClient>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            client,
            root: workspace_root.into(),
            budget: DEFAULT_STEP_BUDGET,
            model: String::new(),
            plan_mode: false,
            messages: Vec::new(),
            queue: VecDeque::new(),
            turns: 0,
            finished: false,
            last_text: String::new(),
            events: None,
        }
    }

    #[must_use]
    pub fn with_step_budget(mut self, budget: usize) -> Self {
        self.budget = budget.max(1);
        self
    }

    /// Conversation so far (for tests and task records).
    #[must_use]
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    fn turn(&mut self) -> AgentStep {
        if self.turns >= self.budget {
            return AgentStep::Done {
                summary: format!(
                    "stopped: step budget of {} model turns reached",
                    self.budget
                ),
            };
        }
        self.turns += 1;
        let request = ChatRequest {
            model: self.model.clone(),
            messages: self.messages.clone(),
            tools: tool_schemas(),
        };
        let events = self.events.clone();
        let mut on_delta = |text: &str| {
            if let Some(events) = &events {
                events.emit(EventKind::MessageDelta {
                    text: text.to_string(),
                });
            }
        };
        let completion = match self.client.complete(&request, &mut on_delta) {
            Ok(completion) => completion,
            Err(error) => {
                return AgentStep::Fail {
                    error: error.to_string(),
                }
            }
        };

        let text = completion.content.trim().to_string();
        self.messages.push(ChatMessage::assistant(
            (!completion.content.is_empty()).then(|| completion.content.clone()),
            completion.tool_calls.clone(),
        ));
        self.finished = completion.tool_calls.is_empty();
        if !text.is_empty() {
            let kind = if self.finished && self.plan_mode {
                EventKind::PlanProposed {
                    markdown: text.clone(),
                }
            } else {
                EventKind::Message { text: text.clone() }
            };
            self.queue.push_back(Queued::Emit(kind));
            self.last_text = text;
        }
        for call in &completion.tool_calls {
            self.queue.push_back(match parse_call(call, &self.root) {
                Ok(request) => Queued::Call {
                    call_id: call.id.clone(),
                    request,
                },
                Err(error) => Queued::Invalid {
                    call_id: call.id.clone(),
                    error,
                },
            });
        }
        self.advance()
    }

    fn advance(&mut self) -> AgentStep {
        while let Some(item) = self.queue.pop_front() {
            match item {
                Queued::Emit(kind) => return AgentStep::Emit(kind),
                Queued::Call { call_id, request } => return AgentStep::Call { call_id, request },
                Queued::Invalid { call_id, error } => {
                    self.messages
                        .push(ChatMessage::tool(call_id, format!("error: {error}")));
                }
            }
        }
        if self.finished {
            let summary = if self.last_text.is_empty() {
                "done".to_string()
            } else {
                crate::runner::truncate(&self.last_text, 500)
            };
            return AgentStep::Done { summary };
        }
        self.turn()
    }
}

impl AgentExecutor for BuiltinExecutor {
    fn id(&self) -> &'static str {
        "builtin"
    }

    fn attach(&mut self, events: &Emitter) {
        self.events = Some(events.clone());
    }

    fn start(&mut self, spec: &TaskSpec) -> AgentStep {
        self.model.clone_from(&spec.model);
        self.plan_mode = spec.mode == "plan";
        self.queue.clear();
        self.turns = 0;
        self.finished = false;
        self.last_text.clear();
        self.messages = vec![
            ChatMessage::system(system_prompt(spec.mode, &self.root)),
            ChatMessage::user(spec.prompt.clone()),
        ];
        self.turn()
    }

    fn next(&mut self, result: Option<&ToolResult>) -> AgentStep {
        if let Some(result) = result {
            let mut content = crate::runner::truncate(&result.output, TOOL_RESULT_LIMIT);
            if !result.ok {
                content = format!("error: {content}");
            }
            self.messages
                .push(ChatMessage::tool(result.call_id.clone(), content));
        }
        self.advance()
    }
}

fn system_prompt(mode: &str, root: &Path) -> String {
    let mode_text = match mode {
        "plan" => "PLAN mode: investigate with read-only tools only (read_file, list_dir, search). Do not write files or run mutating commands. Finish with a Markdown plan.",
        "auto" => "AUTO mode: you may edit files and run development commands; changes happen in an isolated git worktree the user reviews before merging.",
        "bypassPermissions" => "BYPASS mode: permission prompts are disabled; be careful and conservative.",
        _ => "MANUAL mode: mutating actions (writes, patches, commands) need the user's approval and may be denied; if denied, adapt instead of retrying.",
    };
    format!(
        "You are Next Agent, a coding agent working in the workspace at {root}.\n\
         {mode_text}\n\
         Use the provided tools for every file or system interaction. Paths may be \
         relative to the workspace root. Keep answers concise; when done, reply \
         without tool calls and summarize what you did.",
        root = root.display()
    )
}

/// `OpenAI` function schemas for the M1 tool set.
#[must_use]
pub fn tool_schemas() -> Vec<Value> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }
            }
        })
    };
    let path = json!({"type": "string", "description": "File or directory path (absolute or workspace-relative)"});
    vec![
        tool(
            "read_file",
            "Read a UTF-8 text file.",
            json!({"path": path}),
            &["path"],
        ),
        tool(
            "list_dir",
            "List a directory (directories end with /).",
            json!({"path": path}),
            &[],
        ),
        tool(
            "search",
            "Regex search over files (respects .gitignore). Returns path:line:text.",
            json!({"pattern": {"type": "string"}, "path": path}),
            &["pattern"],
        ),
        tool(
            "write_file",
            "Create or overwrite a file with the given contents.",
            json!({"path": path, "contents": {"type": "string"}}),
            &["path", "contents"],
        ),
        tool(
            "apply_patch",
            "Apply a unified diff (---/+++ headers and @@ hunks) to one file.",
            json!({"path": path, "patch": {"type": "string"}}),
            &["path", "patch"],
        ),
        tool(
            "run_command",
            "Run a shell command; returns combined output and exit status.",
            json!({"command": {"type": "string"}, "cwd": path}),
            &["command"],
        ),
        tool(
            "http_fetch",
            "HTTP GET a URL and return the body.",
            json!({"url": {"type": "string"}}),
            &["url"],
        ),
    ]
}

/// Turn a model tool call into a broker request, resolving relative paths.
///
/// # Errors
/// Returns a message for unknown tools, malformed JSON or missing arguments.
pub fn parse_call(call: &ToolCall, root: &Path) -> Result<ToolRequest, String> {
    let raw = call.function.arguments.trim();
    let args: Value = if raw.is_empty() {
        json!({})
    } else {
        serde_json::from_str(raw).map_err(|error| format!("arguments are not JSON: {error}"))?
    };
    let text = |key: &str| -> Result<String, String> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing string argument `{key}`"))
    };
    let path_or_root = |key: &str| -> PathBuf {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map_or_else(|| root.to_path_buf(), |value| resolve(root, value))
    };
    let path = |key: &str| text(key).map(|value| resolve(root, &value));
    Ok(match call.function.name.as_str() {
        "read_file" => ToolRequest::Read {
            path: path("path")?,
        },
        "list_dir" => ToolRequest::ListDir {
            path: path_or_root("path"),
        },
        "search" => ToolRequest::Search {
            pattern: text("pattern")?,
            path: path_or_root("path"),
        },
        "write_file" => ToolRequest::Write {
            path: path("path")?,
            contents: text("contents")?,
        },
        "apply_patch" => ToolRequest::ApplyPatch {
            path: path("path")?,
            patch: text("patch")?,
        },
        "run_command" => ToolRequest::Exec {
            command: text("command")?,
            cwd: path_or_root("cwd"),
        },
        "http_fetch" => ToolRequest::Network { url: text("url")? },
        other => return Err(format!("unknown tool `{other}`")),
    })
}

fn resolve(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{Completion, FunctionCall, LlmError};
    use std::sync::Mutex;

    fn call(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: args.into(),
            },
        }
    }

    /// Replays completions and records the requests it saw.
    struct Scripted {
        replies: Mutex<VecDeque<Result<Completion, LlmError>>>,
        seen: Mutex<Vec<ChatRequest>>,
    }

    impl Scripted {
        fn new(replies: Vec<Result<Completion, LlmError>>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.into()),
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl CompletionClient for Scripted {
        fn complete(
            &self,
            request: &ChatRequest,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<Completion, LlmError> {
            self.seen.lock().expect("lock").push(request.clone());
            let reply = self
                .replies
                .lock()
                .expect("lock")
                .pop_front()
                .unwrap_or_else(|| Ok(Completion::default()));
            if let Ok(completion) = &reply {
                on_delta(&completion.content);
            }
            reply
        }
    }

    fn spec(mode: &'static str) -> TaskSpec {
        TaskSpec {
            id: crate::task::TaskId::from_seed("builtin"),
            prompt: "list files".into(),
            mode,
            model: "test-model".into(),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn text(content: &str) -> Result<Completion, LlmError> {
        Ok(Completion {
            content: content.into(),
            ..Completion::default()
        })
    }

    #[test]
    fn tool_call_round_trip_then_done() {
        let client = Scripted::new(vec![
            Ok(Completion {
                content: String::new(),
                tool_calls: vec![call("c1", "list_dir", "{\"path\":\"src\"}")],
                finish_reason: Some("tool_calls".into()),
            }),
            text("Two files."),
        ]);
        let mut executor = BuiltinExecutor::new(client.clone(), "/w");
        assert_eq!(
            executor.start(&spec("auto")),
            AgentStep::Call {
                call_id: "c1".into(),
                request: ToolRequest::ListDir {
                    path: "/w/src".into()
                }
            }
        );
        let step = executor.next(Some(&ToolResult::ok("c1", "a.rs\nb.rs")));
        assert_eq!(
            step,
            AgentStep::Emit(EventKind::Message {
                text: "Two files.".into()
            })
        );
        assert_eq!(
            executor.next(None),
            AgentStep::Done {
                summary: "Two files.".into()
            }
        );
        let seen = client.seen.lock().expect("lock");
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].model, "test-model");
        assert_eq!(seen[0].tools.len(), 7);
        let last = seen[1].messages.last().expect("tool message");
        assert_eq!(last.role, "tool");
        assert_eq!(last.tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn router_errors_fail_the_task() {
        let client = Scripted::new(vec![Err(LlmError::Unauthorized)]);
        let mut executor = BuiltinExecutor::new(client, "/w");
        assert!(matches!(
            executor.start(&spec("manual")),
            AgentStep::Fail { error } if error.contains("401")
        ));
    }

    #[test]
    fn invalid_calls_are_answered_without_reaching_the_broker() {
        let client = Scripted::new(vec![
            Ok(Completion {
                tool_calls: vec![call("bad", "rm_rf", "{}")],
                ..Completion::default()
            }),
            text("ok"),
        ]);
        let mut executor = BuiltinExecutor::new(client.clone(), "/w");
        assert_eq!(
            executor.start(&spec("auto")),
            AgentStep::Emit(EventKind::Message { text: "ok".into() })
        );
        let seen = client.seen.lock().expect("lock");
        let answer = seen[1].messages.last().expect("tool message");
        assert!(answer
            .content
            .as_deref()
            .is_some_and(|c| c.contains("unknown tool")));
    }

    #[test]
    fn budget_stops_runaway_loops() {
        let looping: Vec<_> = (0..5)
            .map(|i| {
                Ok(Completion {
                    tool_calls: vec![call(&format!("c{i}"), "read_file", "{\"path\":\"a\"}")],
                    ..Completion::default()
                })
            })
            .collect();
        let mut executor = BuiltinExecutor::new(Scripted::new(looping), "/w").with_step_budget(2);
        assert!(matches!(
            executor.start(&spec("auto")),
            AgentStep::Call { .. }
        ));
        assert!(matches!(
            executor.next(Some(&ToolResult::ok("c0", "x"))),
            AgentStep::Call { .. }
        ));
        assert!(matches!(
            executor.next(Some(&ToolResult::ok("c1", "x"))),
            AgentStep::Done { summary } if summary.contains("budget")
        ));
    }

    #[test]
    fn plan_mode_final_answer_is_a_plan() {
        let mut executor = BuiltinExecutor::new(Scripted::new(vec![text("# Plan\n1. x")]), "/w");
        assert_eq!(
            executor.start(&spec("plan")),
            AgentStep::Emit(EventKind::PlanProposed {
                markdown: "# Plan\n1. x".into()
            })
        );
        assert!(executor.messages()[0]
            .content
            .as_deref()
            .is_some_and(|prompt| prompt.contains("PLAN mode")));
    }

    #[test]
    fn parse_call_covers_every_tool() {
        let root = Path::new("/w");
        let cases = [
            (call("1", "read_file", r#"{"path":"a.rs"}"#), "read_file"),
            (call("2", "list_dir", ""), "list_dir"),
            (call("3", "search", r#"{"pattern":"x"}"#), "search"),
            (
                call("4", "write_file", r#"{"path":"/abs","contents":"c"}"#),
                "write_file",
            ),
            (
                call("5", "apply_patch", r#"{"path":"a","patch":"p"}"#),
                "apply_patch",
            ),
            (
                call("6", "run_command", r#"{"command":"ls"}"#),
                "run_command",
            ),
            (
                call("7", "http_fetch", r#"{"url":"https://x"}"#),
                "http_fetch",
            ),
        ];
        for (tool_call, name) in cases {
            let request = parse_call(&tool_call, root).expect(name);
            assert_eq!(request.name(), name);
        }
        assert_eq!(
            parse_call(&call("1", "read_file", r#"{"path":"a.rs"}"#), root),
            Ok(ToolRequest::Read {
                path: "/w/a.rs".into()
            })
        );
        assert!(parse_call(&call("1", "read_file", "{}"), root).is_err());
        assert!(parse_call(&call("1", "read_file", "{nope"), root).is_err());
    }
}
