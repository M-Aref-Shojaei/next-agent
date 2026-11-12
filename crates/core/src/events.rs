//! Streaming events emitted by executors and consumed by the UI.

use crate::task::TaskId;

/// Envelope version; bump on breaking changes (mirrors `packages/contracts`).
pub const EVENT_VERSION: u32 = 1;

/// One step in a task's timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub task_id: TaskId,
    /// Monotonic per task, starting at 0.
    pub seq: u64,
    pub kind: EventKind,
}

/// Event payloads. Keep variants additive; each maps 1:1 to a `kind` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    TaskStarted {
        mode: &'static str,
        executor: String,
    },
    MessageDelta {
        text: String,
    },
    Message {
        text: String,
    },
    PlanProposed {
        markdown: String,
    },
    ToolRequested {
        call_id: String,
        tool: String,
    },
    PermissionDecided {
        call_id: String,
        decision: String,
        reason: String,
    },
    ApprovalRequested {
        approval_id: String,
        call_id: String,
        summary: String,
    },
    ApprovalResolved {
        approval_id: String,
        choice: String,
    },
    ToolFinished {
        call_id: String,
        ok: bool,
    },
    TaskCompleted {
        summary: String,
    },
    TaskFailed {
        error: String,
    },
    TaskCancelled,
}

impl EventKind {
    /// Wire name, matching `docs/architecture/Event Contracts.md`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::TaskStarted { .. } => "task_started",
            Self::MessageDelta { .. } => "message_delta",
            Self::Message { .. } => "message",
            Self::PlanProposed { .. } => "plan_proposed",
            Self::ToolRequested { .. } => "tool_requested",
            Self::PermissionDecided { .. } => "permission_decided",
            Self::ApprovalRequested { .. } => "approval_requested",
            Self::ApprovalResolved { .. } => "approval_resolved",
            Self::ToolFinished { .. } => "tool_finished",
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::TaskCancelled => "task_cancelled",
        }
    }
}

/// Assigns sequence numbers so executors cannot reorder or skip events.
#[derive(Debug)]
pub struct EventLog {
    task_id: TaskId,
    next_seq: u64,
}

impl EventLog {
    #[must_use]
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            next_seq: 0,
        }
    }

    /// Append one event, returning it with its assigned sequence.
    pub fn push(&mut self, kind: EventKind) -> AgentEvent {
        let event = AgentEvent {
            task_id: self.task_id.clone(),
            seq: self.next_seq,
            kind,
        };
        self.next_seq += 1;
        event
    }

    #[must_use]
    pub fn len(&self) -> u64 {
        self.next_seq
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.next_seq == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_are_monotonic_from_zero() {
        let mut log = EventLog::new(TaskId::from_seed("t"));
        assert!(log.is_empty());
        let a = log.push(EventKind::Message { text: "hi".into() });
        let b = log.push(EventKind::TaskCancelled);
        assert_eq!((a.seq, b.seq), (0, 1));
        assert_eq!(log.len(), 2);
        assert!(!log.is_empty());
        assert_eq!(a.task_id, b.task_id);
    }

    #[test]
    fn kind_names_match_contract_doc() {
        assert_eq!(
            EventKind::TaskStarted {
                mode: "auto",
                executor: "mock".into()
            }
            .kind(),
            "task_started"
        );
        assert_eq!(
            EventKind::ToolRequested {
                call_id: "c1".into(),
                tool: "read_file".into()
            }
            .kind(),
            "tool_requested"
        );
        assert_eq!(EventKind::TaskCancelled.kind(), "task_cancelled");
    }

    #[test]
    fn contract_version_is_one() {
        assert_eq!(EVENT_VERSION, 1);
    }
}
