//! Agent execution abstraction.
//!
//! An executor never touches the filesystem or shell. It proposes
//! [`ToolRequest`]s; the broker applies the permission engine and executes
//! them, feeding [`ToolResult`](crate::tools::ToolResult)s back in.
//!
//! The trait is a synchronous step machine driven by
//! [`run_task_with`](crate::runner::run_task_with) on a dedicated (blocking)
//! thread. The async-facing surface from the spec — `start` returning an event
//! stream, `cancel`, `respond(approval_id, decision)` — is
//! [`spawn_task`](crate::control::spawn_task) + [`TaskHandle`](crate::control::TaskHandle).

use crate::events::{Emitter, EventKind};
use crate::task::TaskId;
use crate::tools::ToolRequest;

/// What the agent is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: TaskId,
    pub prompt: String,
    /// Mode label passed to the model (also used in `task_started`).
    pub mode: &'static str,
    pub model: String,
}

/// One step produced by an executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStep {
    /// Progress worth showing in the timeline.
    Emit(EventKind),
    /// A side effect the broker must evaluate and execute.
    Call {
        call_id: String,
        request: ToolRequest,
    },
    /// The agent is finished.
    Done { summary: String },
    /// The agent cannot continue (e.g. the LLM endpoint failed).
    Fail { error: String },
}

/// Executor contract. Implementations: `MockExecutor` (tests/UI dev) and the
/// built-in tool loop; upstream engines arrive as adapters in M2.
pub trait AgentExecutor {
    /// Identifier recorded in `task_started` events.
    fn id(&self) -> &'static str;

    /// Called once before [`AgentExecutor::start`] with the task's event
    /// emitter, for executors that stream progress (e.g. `message_delta`)
    /// while a step is still being computed. Default: ignore.
    fn attach(&mut self, events: &Emitter) {
        let _ = events;
    }

    /// First step of a task.
    fn start(&mut self, spec: &TaskSpec) -> AgentStep;

    /// Agent-profile restriction for a call this executor just proposed. The
    /// runner hands it to the broker, which applies it on top of the
    /// permission engine's decision; it can only tighten. Default: none.
    fn floor(&self, call_id: &str, request: &ToolRequest) -> Option<crate::runner::Floor> {
        let _ = (call_id, request);
        None
    }

    /// Next step, given the result of the previous [`AgentStep::Call`].
    ///
    /// `result` is `None` right after a non-call step.
    fn next(&mut self, result: Option<&crate::tools::ToolResult>) -> AgentStep;
}

/// Scripted executor: replays a fixed list of steps.
///
/// Used by the UI before a router is configured and by integration tests. If a
/// scripted call fails, the remaining calls are skipped and the task ends with
/// a summary of what happened.
#[derive(Debug, Default)]
pub struct MockExecutor {
    script: Vec<AgentStep>,
    cursor: usize,
    completed_calls: usize,
    failures: usize,
}

impl MockExecutor {
    #[must_use]
    pub fn new(script: Vec<AgentStep>) -> Self {
        Self {
            script,
            ..Self::default()
        }
    }
}

impl AgentExecutor for MockExecutor {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn start(&mut self, spec: &TaskSpec) -> AgentStep {
        self.cursor = 0;
        if self.script.is_empty() {
            return AgentStep::Done {
                summary: "nothing to do".into(),
            };
        }
        AgentStep::Emit(EventKind::Message {
            text: format!("(mock) starting {}", spec.prompt),
        })
    }

    fn next(&mut self, result: Option<&crate::tools::ToolResult>) -> AgentStep {
        if let Some(result) = result {
            if !result.ok {
                self.failures += 1;
            }
            self.completed_calls += 1;
            if self.failures > 0 {
                return AgentStep::Done {
                    summary: format!("{} tool call(s) failed", self.failures),
                };
            }
        }
        let Some(step) = self.script.get(self.cursor).cloned() else {
            return AgentStep::Done {
                summary: format!("{} tool call(s) executed", self.completed_calls),
            };
        };
        self.cursor += 1;
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskId;
    use crate::tools::ToolResult;

    fn spec() -> TaskSpec {
        TaskSpec {
            id: TaskId::from_seed("mock"),
            prompt: "debug the build".into(),
            mode: "auto",
            model: "mock-1".into(),
        }
    }

    #[test]
    fn replays_script_in_order() {
        let call = AgentStep::Call {
            call_id: "c1".into(),
            request: ToolRequest::Read {
                path: "/w/a.txt".into(),
            },
        };
        let mut executor = MockExecutor::new(vec![call.clone()]);
        assert_eq!(executor.id(), "mock");
        assert!(matches!(executor.start(&spec()), AgentStep::Emit(_)));
        assert_eq!(executor.next(None), call);
        assert_eq!(
            executor.next(Some(&ToolResult::ok("c1", "contents"))),
            AgentStep::Done {
                summary: "1 tool call(s) executed".into()
            }
        );
    }

    #[test]
    fn stops_after_failure() {
        let mut executor = MockExecutor::new(vec![AgentStep::Call {
            call_id: "c1".into(),
            request: ToolRequest::Exec {
                command: "false".into(),
                cwd: "/w".into(),
            },
        }]);
        let _ = executor.start(&spec());
        let _ = executor.next(None);
        assert_eq!(
            executor.next(Some(&ToolResult::error("c1", "exit 1"))),
            AgentStep::Done {
                summary: "1 tool call(s) failed".into()
            }
        );
    }

    #[test]
    fn empty_script_finishes_immediately() {
        let mut executor = MockExecutor::new(vec![]);
        assert_eq!(
            executor.start(&spec()),
            AgentStep::Done {
                summary: "nothing to do".into()
            }
        );
    }
}
