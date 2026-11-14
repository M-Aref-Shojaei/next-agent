//! Tests for [`BuiltinExecutor`](super::BuiltinExecutor): the tool loop,
//! profiles and the steps cap, compaction, subagents and the prompt-cache
//! invariants.

use super::*;
use crate::events::AgentEvent;
use crate::llm::{FunctionCall, LlmError};
use crate::runner::{run_task, BrokerOutcome, RunContext, ToolBroker};
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

type Handler = Box<dyn FnMut(&ChatRequest) -> Result<Completion, LlmError> + Send>;

/// Answers each request with a closure (to route parent vs child vs
/// summariser requests) and records them.
struct Routed {
    handler: Mutex<Handler>,
    seen: Mutex<Vec<ChatRequest>>,
}

impl Routed {
    fn new(
        handler: impl FnMut(&ChatRequest) -> Result<Completion, LlmError> + Send + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            handler: Mutex::new(Box::new(handler)),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<ChatRequest> {
        self.seen.lock().expect("lock").clone()
    }
}

impl CompletionClient for Routed {
    fn complete(
        &self,
        request: &ChatRequest,
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        self.seen.lock().expect("lock").push(request.clone());
        (self.handler.lock().expect("lock"))(request)
    }
}

/// Allows everything except a `Deny` floor; records calls and floors.
#[derive(Default)]
struct Recording {
    calls: Vec<(String, String)>,
    floors: Vec<(String, Option<Floor>)>,
}

impl ToolBroker for Recording {
    fn handle(&mut self, call_id: &str, request: &ToolRequest) -> BrokerOutcome {
        self.calls
            .push((call_id.to_string(), request.name().to_string()));
        BrokerOutcome {
            decision: "allow",
            reason: "test".into(),
            result: ToolResult::ok(call_id, format!("output of {call_id}")),
        }
    }

    fn handle_floored(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
        floor: Option<&Floor>,
    ) -> BrokerOutcome {
        self.floors.push((call_id.to_string(), floor.cloned()));
        if let Some(Floor::Deny(reason)) = floor {
            return BrokerOutcome {
                decision: "deny",
                reason: reason.clone(),
                result: ToolResult::error(call_id, reason.clone()),
            };
        }
        self.handle_in(ctx, call_id, request)
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

#[allow(clippy::unnecessary_wraps)]
fn calls(calls: Vec<ToolCall>) -> Result<Completion, LlmError> {
    Ok(Completion {
        tool_calls: calls,
        finish_reason: Some("tool_calls".into()),
        ..Completion::default()
    })
}

fn tool_names(request: &ChatRequest) -> Vec<String> {
    request
        .tools
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

fn system_of(request: &ChatRequest) -> &str {
    request.messages[0].content.as_deref().unwrap_or_default()
}

fn last_user(request: &ChatRequest) -> &str {
    request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .unwrap_or_default()
}

fn kinds(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(|e| e.kind.kind()).collect()
}

const SUMMARY: &str = "## Objective\n- list files\n\n## Next Move\n1. continue";

// --- the loop -----------------------------------------------------------------

#[test]
fn tool_call_round_trip_then_done() {
    let client = Scripted::new(vec![
        calls(vec![call("c1", "list_dir", "{\"path\":\"src\"}")]),
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
    // Eight file/system tools plus `subagent` for the top-level build agent.
    assert_eq!(
        tool_names(&seen[0]),
        [
            "read_file",
            "list_dir",
            "search",
            "write_file",
            "edit",
            "apply_patch",
            "run_command",
            "http_fetch",
            "subagent"
        ]
    );
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
    let client = Scripted::new(vec![calls(vec![call("bad", "rm_rf", "{}")]), text("ok")]);
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
fn budget_stops_runaway_loops_with_a_summary_turn() {
    let looping: Vec<_> = (0..5)
        .map(|i| {
            calls(vec![call(
                &format!("c{i}"),
                "read_file",
                "{\"path\":\"a\"}",
            )])
        })
        .collect();
    let client = Scripted::new(looping);
    let mut executor = BuiltinExecutor::new(client.clone(), "/w").with_step_budget(2);
    assert!(matches!(
        executor.start(&spec("auto")),
        AgentStep::Call { .. }
    ));
    assert!(matches!(
        executor.next(Some(&ToolResult::ok("c0", "x"))),
        AgentStep::Call { .. }
    ));
    // Third request is the no-tools summary turn; its (tool-call) reply is
    // ignored and the task stops.
    assert!(matches!(
        executor.next(Some(&ToolResult::ok("c1", "x"))),
        AgentStep::Done { summary } if summary.contains("budget")
    ));
    let seen = client.seen.lock().expect("lock");
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2].tools, Vec::<Value>::new());
    assert!(last_user(&seen[2]).contains("maximum of 2 model turns"));
}

#[test]
fn steps_cap_summary_is_the_final_answer() {
    let client = Scripted::new(vec![
        calls(vec![call("c0", "read_file", "{\"path\":\"a\"}")]),
        text("Read a; b is left."),
    ]);
    let profile = profiles::parse_profile("short", "---\nsteps: 1\n---\nBe brief.").expect("p");
    let mut executor = BuiltinExecutor::new(client.clone(), "/w").with_profile(profile);
    assert!(matches!(
        executor.start(&spec("auto")),
        AgentStep::Call { .. }
    ));
    assert_eq!(
        executor.next(Some(&ToolResult::ok("c0", "x"))),
        AgentStep::Emit(EventKind::Message {
            text: "Read a; b is left.".into()
        })
    );
    assert_eq!(
        executor.next(None),
        AgentStep::Done {
            summary: "Read a; b is left.".into()
        }
    );
    let seen = client.seen.lock().expect("lock");
    assert!(system_of(&seen[0]).contains("Be brief."));
    assert!(system_of(&seen[0]).contains("Agent profile: short."));
    assert_eq!(seen[1].tools, Vec::<Value>::new());
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
    assert_eq!(executor.profile_name(), Some("plan"));
}

#[test]
fn plan_profile_hides_and_refuses_mutating_tools() {
    let client = Scripted::new(vec![
        calls(vec![
            call("w", "write_file", r#"{"path":"a","contents":"x"}"#),
            call(
                "e",
                "edit",
                r#"{"path":"a","old_string":"x","new_string":"y"}"#,
            ),
            call("r", "run_command", r#"{"command":"ls"}"#),
            call("ok", "read_file", r#"{"path":"a"}"#),
        ]),
        text("# Plan"),
    ]);
    let mut executor = BuiltinExecutor::new(client.clone(), "/w");
    // Only the read reaches the broker; the others are answered in place.
    assert_eq!(
        executor.start(&spec("plan")),
        AgentStep::Call {
            call_id: "ok".into(),
            request: ToolRequest::Read {
                path: "/w/a".into()
            }
        }
    );
    let _ = executor.next(Some(&ToolResult::ok("ok", "x")));
    let seen = client.seen.lock().expect("lock");
    let names = tool_names(&seen[0]);
    for hidden in ["write_file", "edit", "apply_patch", "run_command"] {
        assert!(!names.contains(&hidden.to_string()), "{hidden} offered");
    }
    assert!(names.contains(&"subagent".to_string()));
    let refused: Vec<&str> = seen[1]
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.content.as_deref())
        .filter(|c| c.contains("not available to the `plan` agent"))
        .collect();
    assert_eq!(refused.len(), 3);
}

#[test]
fn profile_permissions_become_broker_floors() {
    let profile = profiles::parse_profile(
        "careful",
        "---\npermissions:\n  run_command: ask\n  http_fetch: deny\n---\n",
    )
    .expect("profile");
    let mut executor =
        BuiltinExecutor::new(Scripted::new(vec![text("hi")]), "/w").with_profile(profile);
    let _ = executor.start(&spec("auto"));
    let exec = ToolRequest::Exec {
        command: "make".into(),
        cwd: "/w".into(),
    };
    assert!(matches!(executor.floor("c1", &exec), Some(Floor::Ask(_))));
    let read = ToolRequest::Read {
        path: "/w/a".into(),
    };
    assert_eq!(executor.floor("c2", &read), None);
    assert!(!tool_names(&ChatRequest {
        model: String::new(),
        messages: Vec::new(),
        tools: executor.offered_tools().to_vec()
    })
    .contains(&"http_fetch".to_string()));
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
        (
            call(
                "8",
                "edit",
                r#"{"path":"a","old_string":"x","new_string":"y","replace_all":true}"#,
            ),
            "edit",
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
    assert!(matches!(
        parse_call(
            &call(
                "8",
                "edit",
                r#"{"path":"a","old_string":"x","new_string":"y"}"#
            ),
            root
        ),
        Ok(ToolRequest::Edit {
            replace_all: false,
            ..
        })
    ));
    assert!(parse_call(&call("1", "read_file", "{}"), root).is_err());
    assert!(parse_call(&call("1", "read_file", "{nope"), root).is_err());
}

// --- compaction ---------------------------------------------------------------

fn is_summary_request(request: &ChatRequest) -> bool {
    last_user(request).contains("into a structured summary")
        || last_user(request).contains("Update the existing checkpoint")
}

#[test]
fn compaction_triggers_at_the_ceiling_and_keeps_the_system_prompt() {
    // 8k window → ceiling 6,800 tokens. Each tool result is ~2,500 tokens.
    let mut round = 0;
    let client = Routed::new(move |request| {
        if is_summary_request(request) {
            return text(SUMMARY);
        }
        round += 1;
        if round <= 4 {
            calls(vec![call(
                &format!("c{round}"),
                "read_file",
                "{\"path\":\"a\"}",
            )])
        } else {
            text("finished")
        }
    });
    let mut executor =
        BuiltinExecutor::new(client.clone(), "/w").with_compaction(CompactionConfig {
            window: 8_000,
            keep: 1_000,
        });
    let mut broker = BigOutput;
    let events = run_task(&mut executor, &mut broker, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let compacted: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| e.kind.kind() == "context_compacted")
        .collect();
    assert!(!compacted.is_empty(), "{:?}", kinds(&events));
    assert_eq!(compacted[0].data()["ok"], true);
    assert_eq!(compacted[0].data()["reason"], "auto");
    assert!(executor.compactions() >= 1);

    let seen = client.seen();
    let main: Vec<&ChatRequest> = seen.iter().filter(|r| !is_summary_request(r)).collect();
    let system = system_of(main[0]).to_string();
    for request in &main {
        assert_eq!(
            system_of(request),
            system,
            "system prompt must be byte-stable"
        );
        validate_transcript(&request.messages).expect("invariants");
    }
    // After compaction the conversation opens with the checkpoint as a user
    // message right after the unchanged system prompt.
    let after = main
        .iter()
        .find(|r| {
            r.messages[1]
                .content
                .as_deref()
                .is_some_and(|c| c.starts_with("<conversation-checkpoint>"))
        })
        .expect("a request after compaction");
    assert_eq!(after.messages[1].role, "user");
    assert!(after.messages[1]
        .content
        .as_deref()
        .is_some_and(|c| c.contains("## Objective")));
}

/// Broker returning ~10k chars per call so the prompt grows quickly.
struct BigOutput;
impl ToolBroker for BigOutput {
    fn handle(&mut self, call_id: &str, _request: &ToolRequest) -> BrokerOutcome {
        BrokerOutcome {
            decision: "allow",
            reason: "test".into(),
            result: ToolResult::ok(call_id, "y".repeat(10_000)),
        }
    }
}

#[test]
fn provider_overflow_compacts_and_retries_once() {
    let mut main_calls = 0;
    let client = Routed::new(move |request| {
        if is_summary_request(request) {
            return text(SUMMARY);
        }
        main_calls += 1;
        match main_calls {
            1 | 2 => calls(vec![call(
                &format!("c{main_calls}"),
                "read_file",
                "{\"path\":\"a\"}",
            )]),
            3 => Err(LlmError::Http {
                status: 400,
                body: "prompt is too long: 300000 tokens > 200000 maximum".into(),
            }),
            _ => text("recovered"),
        }
    });
    let mut executor = BuiltinExecutor::new(client.clone(), "/w").with_context_window(0);
    let events = run_task(
        &mut executor,
        &mut Recording::default(),
        &spec("auto"),
        &|| false,
    );
    assert_eq!(
        events.last().map(|e| e.kind.clone()),
        Some(EventKind::TaskCompleted {
            summary: "recovered".into()
        }),
        "{:?}",
        events.iter().map(AgentEvent::data).collect::<Vec<_>>()
    );
    let compacted = events
        .iter()
        .find(|e| e.kind.kind() == "context_compacted")
        .expect("compacted");
    assert_eq!(compacted.data()["reason"], "overflow");
    // A second overflow in the same turn fails the task instead of looping.
    let mut first = true;
    let client = Routed::new(move |request| {
        if is_summary_request(request) {
            return text(SUMMARY);
        }
        if std::mem::take(&mut first) {
            return calls(vec![call("c1", "read_file", "{\"path\":\"a\"}")]);
        }
        Err(LlmError::Http {
            status: 413,
            body: String::new(),
        })
    });
    let mut executor = BuiltinExecutor::new(client, "/w").with_context_window(0);
    let events = run_task(
        &mut executor,
        &mut Recording::default(),
        &spec("auto"),
        &|| false,
    );
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_failed"));
}

#[test]
fn automatic_compaction_stops_after_repeated_failures() {
    let summaries = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&summaries);
    let mut n = 0;
    let client = Routed::new(move |request| {
        if is_summary_request(request) || last_user(request) == compaction::NUDGE {
            *counter.lock().expect("lock") += 1;
            return text("not a summary");
        }
        n += 1;
        if n <= 6 {
            calls(vec![call(
                &format!("c{n}"),
                "read_file",
                "{\"path\":\"a\"}",
            )])
        } else {
            text("done")
        }
    });
    let mut executor = BuiltinExecutor::new(client, "/w").with_compaction(CompactionConfig {
        window: 8_000,
        keep: 1_000,
    });
    let events = run_task(&mut executor, &mut BigOutput, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let failed = events
        .iter()
        .filter(|e| e.kind.kind() == "context_compacted" && e.data()["ok"] == false)
        .count();
    assert_eq!(failed, MAX_AUTO_COMPACTION_FAILURES);
    // Two attempts × (request + nudge).
    assert_eq!(*summaries.lock().expect("lock"), 4);
}

#[test]
fn summary_client_writes_the_checkpoint() {
    let summariser = Scripted::new(vec![text(SUMMARY)]);
    let main = Routed::new({
        let mut n = 0;
        move |_request| {
            n += 1;
            if n <= 3 {
                calls(vec![call(
                    &format!("c{n}"),
                    "read_file",
                    "{\"path\":\"a\"}",
                )])
            } else {
                text("done")
            }
        }
    });
    let mut executor = BuiltinExecutor::new(main.clone(), "/w")
        .with_compaction(CompactionConfig {
            window: 8_000,
            keep: 1_000,
        })
        .with_summary_client(summariser.clone(), Some("summary-model".into()));
    let events = run_task(&mut executor, &mut BigOutput, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let summaries = summariser.seen.lock().expect("lock");
    assert!(!summaries.is_empty());
    assert_eq!(summaries[0].model, "summary-model");
    assert!(main.seen().iter().all(|r| !is_summary_request(r)));
}

// --- prompt-cache invariants ----------------------------------------------------

#[test]
fn validate_transcript_rejects_each_violation() {
    let system = ChatMessage::system("s");
    let user = ChatMessage::user("u");
    let with_call = ChatMessage::assistant(None, vec![call("c1", "read_file", "{}")]);
    let answer = ChatMessage::tool("c1", "x");
    let reply = ChatMessage::assistant(Some("ok".into()), Vec::new());
    assert!(validate_transcript(&[
        system.clone(),
        user.clone(),
        with_call.clone(),
        answer.clone(),
        reply.clone()
    ])
    .is_ok());
    let cases: Vec<(Vec<ChatMessage>, &str)> = vec![
        (vec![], "empty"),
        (vec![user.clone()], "system prompt"),
        (vec![system.clone(), reply.clone()], "user message"),
        (vec![system.clone(), user.clone(), user.clone()], "two user"),
        (
            vec![system.clone(), user.clone(), reply.clone(), reply.clone()],
            "two assistant",
        ),
        (
            vec![system.clone(), user.clone(), ChatMessage::system("again")],
            "second system",
        ),
        (
            vec![system.clone(), user.clone(), with_call.clone()],
            "unanswered",
        ),
        (
            vec![
                system.clone(),
                user.clone(),
                with_call.clone(),
                user.clone(),
            ],
            "before every tool call",
        ),
        (
            vec![
                system.clone(),
                user.clone(),
                with_call.clone(),
                answer.clone(),
                answer.clone(),
            ],
            "answers no pending",
        ),
        (
            vec![system, user, ChatMessage::tool("zz", "x")],
            "answers no pending",
        ),
    ];
    for (messages, needle) in cases {
        let error = validate_transcript(&messages).expect_err(needle);
        assert!(error.contains(needle), "{needle}: {error}");
    }
}

#[test]
fn every_request_of_a_long_run_keeps_the_invariants() {
    // Parallel calls, an invalid call, compaction and the steps cap.
    let mut n = 0;
    let client = Routed::new(move |request| {
        if is_summary_request(request) {
            return text(SUMMARY);
        }
        n += 1;
        calls(vec![
            call(&format!("a{n}"), "read_file", "{\"path\":\"a\"}"),
            call(&format!("b{n}"), "nope", "{}"),
            call(&format!("c{n}"), "search", "{\"pattern\":\"x\"}"),
        ])
    });
    let mut executor = BuiltinExecutor::new(client.clone(), "/w")
        .with_step_budget(6)
        .with_compaction(CompactionConfig {
            window: 8_000,
            keep: 1_000,
        });
    let events = run_task(&mut executor, &mut BigOutput, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let seen = client.seen();
    let main: Vec<&ChatRequest> = seen.iter().filter(|r| !is_summary_request(r)).collect();
    assert_eq!(main.len(), 7, "six turns plus the summary turn");
    let system = system_of(main[0]).to_string();
    let tools = main[0].tools.clone();
    for (index, request) in main.iter().enumerate() {
        assert_eq!(system_of(request), system);
        validate_transcript(&request.messages).unwrap_or_else(|e| panic!("request {index}: {e}"));
        if index < 6 {
            assert_eq!(request.tools, tools, "tool list is stable for the cache");
        }
        for pair in request.messages.windows(2) {
            assert!(
                !(pair[0].role == pair[1].role && pair[0].role != "tool"),
                "consecutive {} messages",
                pair[0].role
            );
        }
        // Nothing but the system prompt has the system role.
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|m| m.role == "system")
                .count(),
            1
        );
    }
    assert!(events.iter().any(|e| e.kind.kind() == "context_compacted"));
}

#[test]
fn a_changed_system_prompt_fails_before_sending() {
    let client = Scripted::new(vec![
        calls(vec![call("c1", "read_file", "{\"path\":\"a\"}")]),
        text("never sent"),
    ]);
    let mut executor = BuiltinExecutor::new(client.clone(), "/w");
    let _ = executor.start(&spec("auto"));
    executor.messages[0].content = Some("tampered".into());
    assert!(matches!(
        executor.next(Some(&ToolResult::ok("c1", "x"))),
        AgentStep::Fail { error } if error.contains("system prompt changed")
    ));
    assert_eq!(client.seen.lock().expect("lock").len(), 1);
}

// --- subagents ----------------------------------------------------------------

fn is_child(request: &ChatRequest) -> bool {
    system_of(request).contains("You are a subagent")
}

fn child_profile(request: &ChatRequest) -> String {
    let system = system_of(request);
    system
        .split("Agent profile: ")
        .nth(1)
        .and_then(|rest| rest.split('.').next())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn subagent_calls_go_through_the_same_broker_and_report_back() {
    let client = Routed::new(|request| {
        if is_child(request) {
            // Child: one read, then a report.
            if request.messages.len() == 2 {
                return calls(vec![call("r1", "read_file", "{\"path\":\"src/lib.rs\"}")]);
            }
            return text("{\"files\": [\"src/lib.rs\"]}");
        }
        if request.messages.len() == 2 {
            return calls(vec![call(
                "s1",
                "subagent",
                r#"{"tasks":[{"goal":"find the entry point","context":"rust crate","agent":"explore","output_schema":{"type":"object","required":["files"]}}]}"#,
            )]);
        }
        text("parent done")
    });
    let mut broker = Recording::default();
    let mut executor = BuiltinExecutor::new(client.clone(), "/w");
    let events = run_task(&mut executor, &mut broker, &spec("auto"), &|| false);
    assert_eq!(
        events.last().map(|e| e.kind.clone()),
        Some(EventKind::TaskCompleted {
            summary: "parent done".into()
        })
    );
    // The child's read went through the broker under a prefixed call id.
    assert_eq!(
        broker.calls,
        [("sub1.r1".to_string(), "read_file".to_string())]
    );
    // Timeline, all under the parent task.
    let timeline = kinds(&events);
    let start = timeline
        .iter()
        .position(|k| *k == "subagent_started")
        .expect("started");
    let finish = timeline
        .iter()
        .position(|k| *k == "subagent_finished")
        .expect("finished");
    assert!(start < finish);
    assert!(timeline[start..finish].contains(&"tool_requested"));
    let finished = &events[finish];
    assert_eq!(finished.data()["status"], "completed");
    assert_eq!(finished.data()["schema_valid"], true);
    assert!(events.iter().all(|e| e.task_id == events[0].task_id));
    // The parent received the compact report as the tool result.
    let seen = client.seen();
    let parent_last = seen.iter().rfind(|r| !is_child(r)).expect("parent");
    let report = parent_last.messages.last().expect("tool");
    assert_eq!(report.tool_call_id.as_deref(), Some("s1"));
    let report: Value =
        serde_json::from_str(report.content.as_deref().expect("json")).expect("json");
    for key in [
        "status",
        "exit_reason",
        "truncated",
        "schema_valid",
        "summary",
    ] {
        assert!(report.get(key).is_some(), "missing {key}");
    }
    assert_eq!(report["exit_reason"], "done");
    // The child ran as explore, in the parent's sandbox root, read-only.
    let child = seen.iter().find(|r| is_child(r)).expect("child request");
    assert_eq!(child_profile(child), "explore");
    assert!(system_of(child).contains("workspace at /w"));
    assert!(last_user(child).contains("find the entry point"));
    assert!(!tool_names(child).contains(&"write_file".to_string()));
    assert!(!tool_names(child).contains(&"subagent".to_string()));
}

#[test]
fn a_child_can_never_do_what_the_parent_could_not() {
    // Parent: `plan` (read-only) spawns `general`, which by itself could write.
    // Parent profile also asks before http_fetch; the child inherits that.
    let parent = profiles::find(&profiles::builtin_profiles(), "plan")
        .expect("plan")
        .tightened_by(
            &profiles::parse_profile("plan", "---\npermissions:\n  http_fetch: ask\n---\n")
                .expect("p"),
        );
    let client = Routed::new(|request| {
        if is_child(request) {
            return match request.messages.len() {
                2 => calls(vec![
                    call("w", "write_file", r#"{"path":"x","contents":"pwned"}"#),
                    call("h", "http_fetch", r#"{"url":"https://example.com"}"#),
                ]),
                _ => text("child done"),
            };
        }
        if request.messages.len() == 2 {
            return calls(vec![call(
                "s1",
                "subagent",
                r#"{"goal":"change things","agent":"general"}"#,
            )]);
        }
        text("parent done")
    });
    let mut broker = Recording::default();
    let mut executor = BuiltinExecutor::new(client.clone(), "/w").with_profile(parent);
    let events = run_task(&mut executor, &mut broker, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    // The write never reached the broker; the fetch did, carrying the
    // parent's Ask floor (the real broker turns that into an approval).
    assert_eq!(
        broker.calls,
        [("sub1.h".to_string(), "http_fetch".to_string())]
    );
    assert_eq!(broker.floors.len(), 1);
    assert_eq!(broker.floors[0].0, "sub1.h");
    assert!(matches!(broker.floors[0].1, Some(Floor::Ask(_))));
    let seen = client.seen();
    let child = seen.iter().find(|r| is_child(r)).expect("child");
    assert!(!tool_names(child).contains(&"write_file".to_string()));
    let child_next = seen.iter().filter(|r| is_child(r)).nth(1).expect("child 2");
    assert!(child_next.messages.iter().any(|m| m
        .content
        .as_deref()
        .is_some_and(|c| c.contains("not available to the `general` agent"))));
}

#[test]
fn deny_floors_from_ancestors_reach_child_calls() {
    let parent = profiles::parse_profile("strict", "---\npermissions:\n  read_file: deny\n---\n")
        .expect("strict");
    let client = Routed::new(|request| {
        if is_child(request) {
            return match request.messages.len() {
                2 => calls(vec![call("r", "search", r#"{"pattern":"x"}"#)]),
                _ => text("ok"),
            };
        }
        if request.messages.len() == 2 {
            return calls(vec![call("s1", "subagent", r#"{"goal":"look"}"#)]);
        }
        text("done")
    });
    let mut executor = BuiltinExecutor::new(client.clone(), "/w").with_profile(parent);
    let mut broker = Recording::default();
    let _ = run_task(&mut executor, &mut broker, &spec("auto"), &|| false);
    let child = client.seen().into_iter().find(is_child).expect("child");
    assert!(!tool_names(&child).contains(&"read_file".to_string()));
    assert_eq!(broker.floors.len(), 1);
    assert_eq!(broker.floors[0].1, None);
    // And the floor function itself denies an ancestor-denied request.
    let mut direct = BuiltinExecutor::new(Scripted::new(vec![text("x")]), "/w").with_profile(
        profiles::parse_profile("strict", "---\npermissions:\n  read_file: deny\n---\n")
            .expect("p"),
    );
    let _ = direct.start(&spec("auto"));
    assert!(matches!(
        direct.floor(
            "c",
            &ToolRequest::Read {
                path: "/w/a".into()
            }
        ),
        Some(Floor::Deny(_))
    ));
}

#[test]
fn children_run_interleaved_within_the_concurrency_limit() {
    let client = Routed::new(|request| {
        if is_child(request) {
            let goal = last_user(request).to_string();
            let tag = if goal.contains("alpha") { "a" } else { "b" };
            let rounds = request.messages.iter().filter(|m| m.role == "tool").count();
            if rounds < 2 {
                return calls(vec![call(
                    &format!("{tag}{rounds}"),
                    "read_file",
                    "{\"path\":\"f\"}",
                )]);
            }
            return text(&format!("{tag} done"));
        }
        if request.messages.len() == 2 {
            return calls(vec![call(
                "s1",
                "subagent",
                r#"{"tasks":[{"goal":"alpha","agent":"explore"},{"goal":"beta","agent":"explore"}]}"#,
            )]);
        }
        text("parent done")
    });
    let mut broker = Recording::default();
    let mut executor = BuiltinExecutor::new(client.clone(), "/w");
    let events = run_task(&mut executor, &mut broker, &spec("auto"), &|| false);
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let ids: Vec<&str> = broker.calls.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["sub1.a0", "sub2.b0", "sub1.a1", "sub2.b1"]);
    let parent_last = client
        .seen()
        .into_iter()
        .rfind(|r| !is_child(r))
        .expect("parent");
    let report: Value = serde_json::from_str(
        parent_last
            .messages
            .last()
            .and_then(|m| m.content.as_deref())
            .expect("report"),
    )
    .expect("json");
    assert_eq!(report["results"].as_array().map(Vec::len), Some(2));
    assert_eq!(report["results"][1]["summary"], "b done");

    // Over the limit: refused without spawning.
    let too_many = Routed::new(|request| {
        if request.messages.len() == 2 {
            return calls(vec![call(
                "s1",
                "subagent",
                r#"{"tasks":[{"goal":"1"},{"goal":"2"},{"goal":"3"},{"goal":"4"}]}"#,
            )]);
        }
        text("ok")
    });
    let mut executor = BuiltinExecutor::new(too_many.clone(), "/w");
    let events = run_task(
        &mut executor,
        &mut Recording::default(),
        &spec("auto"),
        &|| false,
    );
    assert!(!kinds(&events).contains(&"subagent_started"));
    assert!(too_many.seen()[1]
        .messages
        .last()
        .and_then(|m| m.content.as_deref())
        .is_some_and(|c| c.contains("at most 3")));
}

#[test]
fn depth_limit_and_unknown_agents() {
    // With max_depth 2, a general child still cannot spawn (its profile
    // denies it), but a custom `all` profile child can, once.
    let nested = profiles::parse_profile("lead", "---\nmode: all\n---\n").expect("lead");
    let mut all = profiles::builtin_profiles();
    all.push(nested);
    let client = Routed::new(|request| {
        let system = system_of(request);
        if last_user(request).starts_with("deeper") {
            // Grandchild at depth 2 = max: no subagent tool.
            assert!(!tool_names(request).contains(&"subagent".to_string()));
            return text("leaf done");
        }
        if system.contains("Agent profile: lead.") && is_child(request) {
            assert!(tool_names(request).contains(&"subagent".to_string()));
            if request.messages.len() == 2 {
                return calls(vec![call(
                    "n1",
                    "subagent",
                    r#"{"goal":"deeper","agent":"lead"}"#,
                )]);
            }
            return text("lead done");
        }
        if request.messages.len() == 2 {
            return calls(vec![
                call("s1", "subagent", r#"{"goal":"go","agent":"lead"}"#),
                call("s2", "subagent", r#"{"goal":"go","agent":"ghost"}"#),
                call("s3", "subagent", r#"{"goal":"go","agent":"plan"}"#),
            ]);
        }
        text("root done")
    });
    let mut executor = BuiltinExecutor::new(client.clone(), "/w")
        .with_profiles(all)
        .with_subagent_limits(SubagentLimits {
            max_depth: 2,
            ..SubagentLimits::default()
        });
    let events = run_task(
        &mut executor,
        &mut Recording::default(),
        &spec("auto"),
        &|| false,
    );
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let started: Vec<String> = events
        .iter()
        .filter(|e| e.kind.kind() == "subagent_started")
        .map(|e| {
            e.data()["subagent_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(started, ["sub1", "sub1.sub1"]);
    let root_last = client
        .seen()
        .into_iter()
        .rfind(|r| !is_child(r))
        .expect("root");
    let answers: Vec<&str> = root_last
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.content.as_deref())
        .collect();
    assert!(answers
        .iter()
        .any(|a| a.contains("unknown subagent `ghost`")));
    // `plan` is primary-only, so it is not spawnable either.
    assert!(answers
        .iter()
        .any(|a| a.contains("unknown subagent `plan`")));
}

#[test]
fn failed_children_report_errors_without_failing_the_parent() {
    let client = Routed::new(|request| {
        if is_child(request) {
            return Err(LlmError::Unavailable("boom".into()));
        }
        if request.messages.len() == 2 {
            return calls(vec![call("s1", "subagent", r#"{"goal":"x"}"#)]);
        }
        text("handled")
    });
    let mut executor = BuiltinExecutor::new(client.clone(), "/w");
    let events = run_task(
        &mut executor,
        &mut Recording::default(),
        &spec("auto"),
        &|| false,
    );
    assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_completed"));
    let finished = events
        .iter()
        .find(|e| e.kind.kind() == "subagent_finished")
        .expect("finished");
    assert_eq!(finished.data()["status"], "failed");
    assert_eq!(finished.data()["exit_reason"], "error");
}
