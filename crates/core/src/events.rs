//! Streaming events emitted by executors and consumed by the UI.

use crate::store::Store;
use crate::task::TaskId;
use serde::Serialize;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Envelope version; bump on breaking changes (mirrors `packages/contracts`).
pub const EVENT_VERSION: u32 = 1;

/// One step in a task's timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub task_id: TaskId,
    /// Monotonic per task, starting at 0.
    pub seq: u64,
    /// RFC 3339 UTC timestamp assigned when the event was sequenced.
    pub ts: String,
    pub kind: EventKind,
}

impl AgentEvent {
    /// Stable event id (matches the `events.id` column).
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}:{}", self.task_id, self.seq)
    }

    /// The `data` object of the wire envelope.
    #[must_use]
    pub fn data(&self) -> serde_json::Value {
        self.kind.data()
    }

    /// Wire envelope per `docs/architecture/Event Contracts.md`. This is what
    /// the desktop app forwards over its Tauri `Channel`.
    #[must_use]
    pub fn envelope(&self) -> serde_json::Value {
        serde_json::json!({
            "v": EVENT_VERSION,
            "id": self.id(),
            "task_id": self.task_id.as_str(),
            "seq": self.seq,
            "ts": self.ts,
            "kind": self.kind.kind(),
            "data": self.data(),
        })
    }
}

/// Event payloads. Keep variants additive; each maps 1:1 to a `kind` string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventKind {
    TaskStarted {
        mode: &'static str,
        executor: String,
        model: String,
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
        args: serde_json::Value,
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
        options: Vec<String>,
    },
    ApprovalResolved {
        approval_id: String,
        choice: String,
        by: String,
    },
    ToolStarted {
        call_id: String,
    },
    ToolOutput {
        call_id: String,
        /// `stdout` or `stderr`.
        stream: String,
        chunk: String,
    },
    ToolFinished {
        call_id: String,
        ok: bool,
        exit_code: Option<i32>,
        /// Truncated tool output for the timeline.
        summary: String,
    },
    /// The built-in loop replaced older history with a checkpoint summary
    /// (or tried to: `ok == false` carries the error).
    ContextCompacted {
        /// `auto` (estimate reached the ceiling) or `overflow` (provider
        /// rejected the prompt as too long).
        reason: String,
        ok: bool,
        tokens_before: u64,
        tokens_after: u64,
        kept_messages: usize,
        summary_chars: usize,
        error: Option<String>,
    },
    /// The work tree was captured before a mutating step (per-step undo).
    StepSnapshot {
        call_id: String,
        seq: i64,
        tree: String,
    },
    /// A subagent started under this task. Its tool calls appear in this
    /// task's timeline with call ids prefixed by `subagent_id`.
    SubagentStarted {
        subagent_id: String,
        parent_call_id: String,
        profile: String,
        goal: String,
        depth: usize,
    },
    SubagentFinished {
        subagent_id: String,
        /// `completed` or `failed`.
        status: String,
        /// `done`, `max_steps`, `error`, `cancelled`.
        exit_reason: String,
        truncated: bool,
        schema_valid: Option<bool>,
        steps: usize,
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
            Self::ToolStarted { .. } => "tool_started",
            Self::ToolOutput { .. } => "tool_output",
            Self::ToolFinished { .. } => "tool_finished",
            Self::ContextCompacted { .. } => "context_compacted",
            Self::StepSnapshot { .. } => "step_snapshot",
            Self::SubagentStarted { .. } => "subagent_started",
            Self::SubagentFinished { .. } => "subagent_finished",
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::TaskCancelled => "task_cancelled",
        }
    }

    /// Payload as a JSON object (`{}` for payload-less kinds).
    #[must_use]
    pub fn data(&self) -> serde_json::Value {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(mut map)) => map
                .remove("data")
                .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
            _ => serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Whether this event ends the task.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::TaskCompleted { .. } | Self::TaskFailed { .. } | Self::TaskCancelled
        )
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
            ts: now_rfc3339(),
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

/// Receives every sequenced event, in order.
///
/// Sinks run in the order they were added; add the persistence sink first so
/// an event is stored before the UI sees it. A sink must not emit events
/// itself (the emitter is locked while delivering).
pub trait EventSink: Send {
    /// Deliver one event.
    ///
    /// # Errors
    /// Returns a description of the failure; the emitter stops delivering the
    /// event to later sinks and the runner fails the task.
    fn deliver(&mut self, event: &AgentEvent) -> Result<(), String>;
}

/// Shared, cloneable handle that sequences events and fans them out to sinks.
///
/// The runner, broker, approver and tool executor all hold clones, so tool
/// output and approval prompts land in the same ordered log as the runner's
/// own events.
#[derive(Clone)]
pub struct Emitter {
    inner: Arc<Mutex<EmitterState>>,
}

struct EmitterState {
    log: EventLog,
    sinks: Vec<Box<dyn EventSink>>,
    error: Option<String>,
}

impl std::fmt::Debug for Emitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("Emitter")
            .field("task_id", &state.log.task_id)
            .field("len", &state.log.len())
            .field("sinks", &state.sinks.len())
            .finish_non_exhaustive()
    }
}

impl Emitter {
    #[must_use]
    pub fn new(task_id: TaskId) -> Self {
        Self {
            inner: Arc::new(Mutex::new(EmitterState {
                log: EventLog::new(task_id),
                sinks: Vec::new(),
                error: None,
            })),
        }
    }

    /// Builder form of [`Emitter::add_sink`].
    #[must_use]
    pub fn with_sink(self, sink: impl EventSink + 'static) -> Self {
        self.add_sink(sink);
        self
    }

    pub fn add_sink(&self, sink: impl EventSink + 'static) {
        self.lock().sinks.push(Box::new(sink));
    }

    /// Sequence and deliver one event.
    pub fn emit(&self, kind: EventKind) -> AgentEvent {
        let mut state = self.lock();
        let event = state.log.push(kind);
        let EmitterState { sinks, error, .. } = &mut *state;
        for sink in sinks.iter_mut() {
            if let Err(problem) = sink.deliver(&event) {
                tracing::error!(seq = event.seq, kind = event.kind.kind(), %problem, "event sink failed");
                error.get_or_insert(problem);
                break;
            }
        }
        event
    }

    /// First sink failure, if any.
    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Number of events emitted so far.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.lock().log.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn task_id(&self) -> TaskId {
        self.lock().log.task_id.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, EmitterState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Persists every event to the `events` table.
#[derive(Debug)]
pub struct StoreSink {
    store: Store,
}

impl StoreSink {
    /// The sink owns its own connection (open the same database file the rest
    /// of the app uses; WAL mode lets them coexist).
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl EventSink for StoreSink {
    fn deliver(&mut self, event: &AgentEvent) -> Result<(), String> {
        self.store
            .append_event(
                event.task_id.as_str(),
                event.seq,
                event.kind.kind(),
                &event.data().to_string(),
            )
            .map_err(|error| format!("persisting event {} failed: {error}", event.seq))
    }
}

/// Collects events in memory (tests, `run_task`).
#[derive(Debug, Clone, Default)]
pub struct Collector {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl Collector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of everything received so far.
    #[must_use]
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl EventSink for Collector {
    fn deliver(&mut self, event: &AgentEvent) -> Result<(), String> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
        Ok(())
    }
}

/// Forwards events over a std channel (e.g. to a thread that relays them to a
/// Tauri `Channel`). A dropped receiver is not an error: the timeline is
/// still persisted and can be replayed.
impl EventSink for std::sync::mpsc::Sender<AgentEvent> {
    fn deliver(&mut self, event: &AgentEvent) -> Result<(), String> {
        let _ = self.send(event.clone());
        Ok(())
    }
}

/// Adapts a closure into a sink, e.g. `FnSink(move |e| channel.send(e.envelope()))`.
pub struct FnSink<F>(pub F);

impl<F> EventSink for FnSink<F>
where
    F: FnMut(&AgentEvent) -> Result<(), String> + Send,
{
    fn deliver(&mut self, event: &AgentEvent) -> Result<(), String> {
        (self.0)(event)
    }
}

/// Current time as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
#[must_use]
pub fn now_rfc3339() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_rfc3339(since.as_secs(), since.subsec_millis())
}

/// Format seconds since the Unix epoch (UTC).
#[must_use]
pub fn format_rfc3339(secs: u64, millis: u32) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Proleptic Gregorian (year, month, day) for days since 1970-01-01, via
/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or_default(),
        u32::try_from(day).unwrap_or_default(),
    )
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
                executor: "mock".into(),
                model: "m".into()
            }
            .kind(),
            "task_started"
        );
        assert_eq!(
            EventKind::ToolRequested {
                call_id: "c1".into(),
                tool: "read_file".into(),
                args: serde_json::json!({})
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

    #[test]
    fn serde_tags_match_kind_names() {
        let samples = [
            EventKind::TaskStarted {
                mode: "auto",
                executor: "mock".into(),
                model: "m".into(),
            },
            EventKind::MessageDelta { text: "a".into() },
            EventKind::Message { text: "a".into() },
            EventKind::PlanProposed {
                markdown: "# p".into(),
            },
            EventKind::ToolRequested {
                call_id: "c".into(),
                tool: "read_file".into(),
                args: serde_json::json!({"path": "/w"}),
            },
            EventKind::PermissionDecided {
                call_id: "c".into(),
                decision: "allow".into(),
                reason: "r".into(),
            },
            EventKind::ApprovalRequested {
                approval_id: "a".into(),
                call_id: "c".into(),
                summary: "s".into(),
                options: vec!["allow".into()],
            },
            EventKind::ApprovalResolved {
                approval_id: "a".into(),
                choice: "allow".into(),
                by: "user".into(),
            },
            EventKind::ToolStarted {
                call_id: "c".into(),
            },
            EventKind::ContextCompacted {
                reason: "auto".into(),
                ok: true,
                tokens_before: 10,
                tokens_after: 2,
                kept_messages: 1,
                summary_chars: 5,
                error: None,
            },
            EventKind::StepSnapshot {
                call_id: "c".into(),
                seq: 1,
                tree: "t".into(),
            },
            EventKind::SubagentStarted {
                subagent_id: "sub1".into(),
                parent_call_id: "c".into(),
                profile: "explore".into(),
                goal: "g".into(),
                depth: 1,
            },
            EventKind::SubagentFinished {
                subagent_id: "sub1".into(),
                status: "completed".into(),
                exit_reason: "done".into(),
                truncated: false,
                schema_valid: None,
                steps: 2,
            },
            EventKind::ToolOutput {
                call_id: "c".into(),
                stream: "stdout".into(),
                chunk: "x".into(),
            },
            EventKind::ToolFinished {
                call_id: "c".into(),
                ok: true,
                exit_code: Some(0),
                summary: String::new(),
            },
            EventKind::TaskCompleted {
                summary: "s".into(),
            },
            EventKind::TaskFailed { error: "e".into() },
            EventKind::TaskCancelled,
        ];
        for sample in samples {
            let value = serde_json::to_value(&sample).expect("serialize");
            assert_eq!(value["kind"], sample.kind(), "{sample:?}");
            assert!(sample.data().is_object(), "{sample:?}");
        }
    }

    #[test]
    fn envelope_matches_contract_shape() {
        let mut log = EventLog::new(TaskId::from_seed("env"));
        let event = log.push(EventKind::ToolFinished {
            call_id: "c1".into(),
            ok: false,
            exit_code: Some(2),
            summary: "boom".into(),
        });
        let envelope = event.envelope();
        assert_eq!(envelope["v"], 1);
        assert_eq!(envelope["seq"], 0);
        assert_eq!(envelope["kind"], "tool_finished");
        assert_eq!(envelope["data"]["exit_code"], 2);
        assert_eq!(envelope["task_id"], event.task_id.as_str());
        assert!(envelope["ts"].as_str().is_some_and(|ts| ts.ends_with('Z')));
        assert_eq!(
            EventKind::TaskCancelled.data(),
            serde_json::json!({}),
            "payload-less kinds still carry an object"
        );
    }

    #[test]
    fn rfc3339_formatting() {
        assert_eq!(format_rfc3339(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_rfc3339(1_794_391_200, 7), "2026-11-11T10:00:00.007Z");
        assert_eq!(format_rfc3339(951_782_400, 0), "2000-02-29T00:00:00.000Z");
    }

    struct Failing;
    impl EventSink for Failing {
        fn deliver(&mut self, _event: &AgentEvent) -> Result<(), String> {
            Err("disk full".into())
        }
    }

    #[test]
    fn emitter_delivers_in_order_and_stops_at_failing_sink() {
        let first = Collector::new();
        let emitter = Emitter::new(TaskId::from_seed("e")).with_sink(first.clone());
        emitter.emit(EventKind::Message { text: "a".into() });
        emitter.emit(EventKind::Message { text: "b".into() });
        let seqs: Vec<u64> = first.events().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, [0, 1]);
        assert!(emitter.error().is_none());

        let after = Collector::new();
        emitter.add_sink(Failing);
        emitter.add_sink(after.clone());
        emitter.emit(EventKind::TaskCancelled);
        assert_eq!(emitter.error().as_deref(), Some("disk full"));
        assert!(
            after.events().is_empty(),
            "later sinks must not see unpersisted events"
        );
        assert_eq!(emitter.len(), 3);
    }
}
