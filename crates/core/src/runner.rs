//! Drives an [`AgentExecutor`] to completion, routing every tool call through
//! a [`ToolBroker`] and recording an ordered event log.

use crate::agent::{AgentExecutor, AgentStep, TaskSpec};
use crate::events::{AgentEvent, Collector, Emitter, EventKind};
use crate::task::TaskStatus;
use crate::tools::{ToolRequest, ToolResult};

/// Per-run context shared by the runner, broker and tool executor.
pub struct RunContext<'a> {
    /// The task's ordered event pipeline.
    pub events: &'a Emitter,
    /// Polled between steps and while tools run; `true` stops the task.
    pub cancelled: &'a dyn Fn() -> bool,
}

impl RunContext<'_> {
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        (self.cancelled)()
    }
}

/// Evaluates and executes tool requests. The real implementation
/// ([`SqliteBroker`](crate::broker::SqliteBroker)) consults the permission
/// engine; tests use scripted brokers.
pub trait ToolBroker {
    /// Decide and execute. Returns the decision label/reason for the timeline
    /// plus the result (a denied call yields `ok == false`).
    fn handle(&mut self, call_id: &str, request: &ToolRequest) -> BrokerOutcome;

    /// Streaming form used by the runner. Implementations emit
    /// `permission_decided` (and any `approval_*`, `tool_started`,
    /// `tool_output` events) themselves, in that order, and stop early when
    /// the context is cancelled.
    ///
    /// The default runs [`ToolBroker::handle`] and then reports the decision.
    fn handle_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> BrokerOutcome {
        let outcome = self.handle(call_id, request);
        ctx.events.emit(EventKind::PermissionDecided {
            call_id: call_id.to_string(),
            decision: outcome.decision.to_string(),
            reason: outcome.reason.clone(),
        });
        outcome
    }
}

/// Broker verdict for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerOutcome {
    pub decision: &'static str,
    pub reason: String,
    pub result: ToolResult,
}

/// Upper bound on steps so a misbehaving executor cannot loop forever.
pub const MAX_STEPS: usize = 200;

/// Longest tool output copied into a `tool_finished` summary.
pub const SUMMARY_LIMIT: usize = 2_000;

/// Run a task, returning every event in order.
pub fn run_task(
    executor: &mut dyn AgentExecutor,
    broker: &mut dyn ToolBroker,
    spec: &TaskSpec,
    cancelled: &dyn Fn() -> bool,
) -> Vec<AgentEvent> {
    let collector = Collector::new();
    let events = Emitter::new(spec.id.clone()).with_sink(collector.clone());
    let ctx = RunContext {
        events: &events,
        cancelled,
    };
    run_task_with(executor, broker, spec, &ctx);
    collector.events()
}

/// Run a task, emitting every event through `ctx.events`. Returns the
/// terminal status (also recorded as the last event).
pub fn run_task_with(
    executor: &mut dyn AgentExecutor,
    broker: &mut dyn ToolBroker,
    spec: &TaskSpec,
    ctx: &RunContext<'_>,
) -> TaskStatus {
    let events = ctx.events;
    events.emit(EventKind::TaskStarted {
        mode: spec.mode,
        executor: executor.id().to_string(),
        model: spec.model.clone(),
    });
    executor.attach(events);

    let mut step = executor.start(spec);
    for _ in 0..MAX_STEPS {
        if let Some(error) = events.error() {
            events.emit(EventKind::TaskFailed {
                error: format!("event pipeline failed: {error}"),
            });
            return TaskStatus::Failed;
        }
        if ctx.is_cancelled() {
            events.emit(EventKind::TaskCancelled);
            return TaskStatus::Cancelled;
        }
        match step {
            AgentStep::Emit(kind) => {
                events.emit(kind);
                step = executor.next(None);
            }
            AgentStep::Call { call_id, request } => {
                events.emit(EventKind::ToolRequested {
                    call_id: call_id.clone(),
                    tool: request.name().to_string(),
                    args: request.args_json(),
                });
                let outcome = broker.handle_in(ctx, &call_id, &request);
                events.emit(EventKind::ToolFinished {
                    call_id,
                    ok: outcome.result.ok,
                    exit_code: outcome.result.exit_code,
                    summary: truncate(&outcome.result.output, SUMMARY_LIMIT),
                });
                if ctx.is_cancelled() {
                    events.emit(EventKind::TaskCancelled);
                    return TaskStatus::Cancelled;
                }
                step = executor.next(Some(&outcome.result));
            }
            AgentStep::Done { summary } => {
                events.emit(EventKind::TaskCompleted { summary });
                return TaskStatus::Completed;
            }
            AgentStep::Fail { error } => {
                events.emit(EventKind::TaskFailed { error });
                return TaskStatus::Failed;
            }
        }
    }

    events.emit(EventKind::TaskFailed {
        error: format!("step limit of {MAX_STEPS} exceeded"),
    });
    TaskStatus::Failed
}

/// Cut `text` to at most `limit` bytes on a char boundary, marking the cut.
#[must_use]
pub fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} bytes truncated]", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::MockExecutor;
    use crate::task::TaskId;
    use std::cell::Cell;

    struct AllowAll;
    impl ToolBroker for AllowAll {
        fn handle(&mut self, call_id: &str, _request: &ToolRequest) -> BrokerOutcome {
            BrokerOutcome {
                decision: "allow",
                reason: "test".into(),
                result: ToolResult::ok(call_id, "done"),
            }
        }
    }

    struct DenyAll;
    impl ToolBroker for DenyAll {
        fn handle(&mut self, call_id: &str, _request: &ToolRequest) -> BrokerOutcome {
            BrokerOutcome {
                decision: "deny",
                reason: "blocked".into(),
                result: ToolResult::error(call_id, "denied"),
            }
        }
    }

    struct Looping;
    impl AgentExecutor for Looping {
        fn id(&self) -> &'static str {
            "loop"
        }
        fn start(&mut self, _spec: &TaskSpec) -> AgentStep {
            AgentStep::Emit(EventKind::Message { text: "x".into() })
        }
        fn next(&mut self, _result: Option<&ToolResult>) -> AgentStep {
            AgentStep::Emit(EventKind::Message { text: "x".into() })
        }
    }

    fn spec() -> TaskSpec {
        TaskSpec {
            id: TaskId::from_seed("run"),
            prompt: "p".into(),
            mode: "auto",
            model: "m".into(),
        }
    }

    fn read_call(id: &str) -> AgentStep {
        AgentStep::Call {
            call_id: id.into(),
            request: ToolRequest::Read {
                path: "/w/a".into(),
            },
        }
    }

    fn kinds(events: &[AgentEvent]) -> Vec<&'static str> {
        events.iter().map(|event| event.kind.kind()).collect()
    }

    #[test]
    fn successful_call_produces_ordered_timeline() {
        let mut executor = MockExecutor::new(vec![read_call("c1")]);
        let events = run_task(&mut executor, &mut AllowAll, &spec(), &|| false);
        assert_eq!(
            kinds(&events),
            [
                "task_started",
                "message",
                "tool_requested",
                "permission_decided",
                "tool_finished",
                "task_completed"
            ]
        );
        let seqs: Vec<u64> = events.iter().map(|event| event.seq).collect();
        assert_eq!(seqs, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn denied_call_ends_task_with_failure_summary() {
        let mut executor = MockExecutor::new(vec![read_call("c1"), read_call("c2")]);
        let events = run_task(&mut executor, &mut DenyAll, &spec(), &|| false);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind.kind() == "tool_requested")
                .count(),
            1
        );
        assert_eq!(
            events.last().map(|event| &event.kind),
            Some(&EventKind::TaskCompleted {
                summary: "1 tool call(s) failed".into()
            })
        );
    }

    #[test]
    fn cancellation_stops_before_next_step() {
        let mut executor = MockExecutor::new(vec![read_call("c1")]);
        let polls = Cell::new(0);
        let cancel_after_first = || {
            polls.set(polls.get() + 1);
            polls.get() > 1
        };
        let events = run_task(&mut executor, &mut AllowAll, &spec(), &cancel_after_first);
        assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_cancelled"));
        assert!(!kinds(&events).contains(&"tool_requested"));
    }

    #[test]
    fn runaway_executor_hits_step_limit() {
        let events = run_task(&mut Looping, &mut AllowAll, &spec(), &|| false);
        assert_eq!(events.last().map(|e| e.kind.kind()), Some("task_failed"));
        assert_eq!(events.len(), MAX_STEPS + 2);
    }

    #[test]
    fn failing_executor_ends_with_task_failed() {
        struct Broken;
        impl AgentExecutor for Broken {
            fn id(&self) -> &'static str {
                "broken"
            }
            fn start(&mut self, _spec: &TaskSpec) -> AgentStep {
                AgentStep::Fail {
                    error: "router down".into(),
                }
            }
            fn next(&mut self, _result: Option<&ToolResult>) -> AgentStep {
                unreachable!()
            }
        }
        let events = run_task(&mut Broken, &mut AllowAll, &spec(), &|| false);
        assert_eq!(kinds(&events), ["task_started", "task_failed"]);
    }

    #[test]
    fn tool_events_carry_args_and_summary() {
        let mut executor = MockExecutor::new(vec![read_call("c1")]);
        let events = run_task(&mut executor, &mut AllowAll, &spec(), &|| false);
        let requested = events
            .iter()
            .find(|e| e.kind.kind() == "tool_requested")
            .expect("requested");
        assert_eq!(requested.data()["args"]["path"], "/w/a");
        let finished = events
            .iter()
            .find(|e| e.kind.kind() == "tool_finished")
            .expect("finished");
        assert_eq!(finished.data()["summary"], "done");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("abc", 5), "abc");
        let cut = truncate("héllo", 2);
        assert!(cut.starts_with('h'));
        assert!(cut.contains("truncated"));
    }

    #[test]
    fn empty_script_completes_without_calls() {
        let mut executor = MockExecutor::new(vec![]);
        let events = run_task(&mut executor, &mut AllowAll, &spec(), &|| false);
        assert_eq!(kinds(&events), ["task_started", "task_completed"]);
    }
}
