//! Drives an [`AgentExecutor`] to completion, routing every tool call through
//! a [`ToolBroker`] and recording an ordered event log.

use crate::agent::{AgentExecutor, AgentStep, TaskSpec};
use crate::events::{AgentEvent, EventKind, EventLog};
use crate::tools::{ToolRequest, ToolResult};

/// Evaluates and executes tool requests. The real implementation (task 3)
/// consults the permission engine; tests use scripted brokers.
pub trait ToolBroker {
    /// Decide and execute. Returns the decision label/reason for the timeline
    /// plus the result (a denied call yields `ok == false`).
    fn handle(&mut self, call_id: &str, request: &ToolRequest) -> BrokerOutcome;
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

/// Run a task, returning every event in order.
pub fn run_task(
    executor: &mut dyn AgentExecutor,
    broker: &mut dyn ToolBroker,
    spec: &TaskSpec,
    cancelled: &dyn Fn() -> bool,
) -> Vec<AgentEvent> {
    let mut log = EventLog::new(spec.id.clone());
    let mut events = vec![log.push(EventKind::TaskStarted {
        mode: spec.mode,
        executor: executor.id().to_string(),
    })];

    let mut step = executor.start(spec);
    for _ in 0..MAX_STEPS {
        if cancelled() {
            events.push(log.push(EventKind::TaskCancelled));
            return events;
        }
        match step {
            AgentStep::Emit(kind) => {
                events.push(log.push(kind));
                step = executor.next(None);
            }
            AgentStep::Call { call_id, request } => {
                events.push(log.push(EventKind::ToolRequested {
                    call_id: call_id.clone(),
                    tool: request.kind().to_string(),
                }));
                let outcome = broker.handle(&call_id, &request);
                events.push(log.push(EventKind::PermissionDecided {
                    call_id: call_id.clone(),
                    decision: outcome.decision.to_string(),
                    reason: outcome.reason.clone(),
                }));
                events.push(log.push(EventKind::ToolFinished {
                    call_id,
                    ok: outcome.result.ok,
                }));
                step = executor.next(Some(&outcome.result));
            }
            AgentStep::Done { summary } => {
                events.push(log.push(EventKind::TaskCompleted { summary }));
                return events;
            }
        }
    }

    events.push(log.push(EventKind::TaskFailed {
        error: format!("step limit of {MAX_STEPS} exceeded"),
    }));
    events
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
        assert_eq!(events.iter().filter(|e| e.kind.kind() == "tool_requested").count(), 1);
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
    fn empty_script_completes_without_calls() {
        let mut executor = MockExecutor::new(vec![]);
        let events = run_task(&mut executor, &mut AllowAll, &spec(), &|| false);
        assert_eq!(kinds(&events), ["task_started", "task_completed"]);
    }
}
