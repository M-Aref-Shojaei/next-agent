//! Live task control: cancellation, the approval round-trip, and the
//! thread-backed task launcher the desktop app drives.
//!
//! Spec surface ([Agent Execution], M1 Step 2) → this module:
//! - `start(task, ctx) -> stream<AgentEvent>` → [`spawn_task`] returning a
//!   [`TaskHandle`] whose [`TaskHandle::events`] receiver yields every event
//!   (already persisted) in order;
//! - `cancel(task_id)` → [`TaskHandle::cancel`] / [`TaskControl::cancel`];
//! - `respond(approval_id, decision)` → [`TaskControl::respond`].
//!
//! [Agent Execution]: ../../../docs/architecture/Agent%20Execution.md

use crate::agent::{AgentExecutor, TaskSpec};
use crate::broker::{describe, Approval, Approver, SqliteBroker, ToolExecutor};
use crate::events::{AgentEvent, Emitter, EventKind, StoreSink};
use crate::runner::{run_task_with, RunContext};
use crate::snapshot::StepSnapshots;
use crate::store::Store;
use crate::task::{TaskId, TaskStatus};
use crate::tools::ToolRequest;
use next_agent_permission_engine::{Context, Mode};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Failures of [`TaskControl::respond`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// No approval with this id is waiting (already answered, or unknown).
    UnknownApproval(String),
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownApproval(id) => write!(f, "no pending approval with id {id}"),
        }
    }
}

impl std::error::Error for ControlError {}

/// Thread-safe control block shared between the UI side and a running task.
#[derive(Debug, Default)]
pub struct TaskControl {
    cancelled: AtomicBool,
    pending: Mutex<HashMap<String, Sender<Approval>>>,
}

impl TaskControl {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Request cancellation. The runner stops before the next step, running
    /// commands are killed, and pending approvals resolve as denied.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Answer a pending approval.
    ///
    /// # Errors
    /// [`ControlError::UnknownApproval`] if nothing is waiting on `approval_id`.
    pub fn respond(&self, approval_id: &str, approval: Approval) -> Result<(), ControlError> {
        let sender = self
            .lock()
            .remove(approval_id)
            .ok_or_else(|| ControlError::UnknownApproval(approval_id.to_string()))?;
        sender
            .send(approval)
            .map_err(|_| ControlError::UnknownApproval(approval_id.to_string()))
    }

    /// Ids of approvals currently waiting for an answer.
    #[must_use]
    pub fn pending_approvals(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.lock().keys().cloned().collect();
        ids.sort();
        ids
    }

    fn register(&self, approval_id: &str) -> Receiver<Approval> {
        let (sender, receiver) = mpsc::channel();
        self.lock().insert(approval_id.to_string(), sender);
        receiver
    }

    fn unregister(&self, approval_id: &str) {
        self.lock().remove(approval_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Sender<Approval>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// [`Approver`] that pauses the tool, emits `approval_requested`, and blocks
/// until [`TaskControl::respond`] answers (or the task is cancelled).
#[derive(Debug)]
pub struct ChannelApprover {
    control: Arc<TaskControl>,
    events: Emitter,
    poll: Duration,
}

impl ChannelApprover {
    #[must_use]
    pub fn new(control: Arc<TaskControl>, events: Emitter) -> Self {
        Self {
            control,
            events,
            poll: Duration::from_millis(25),
        }
    }
}

impl Approver for ChannelApprover {
    fn records_events(&self) -> bool {
        true
    }

    fn approve(&mut self, call_id: &str, request: &ToolRequest, reason: &str) -> Approval {
        let approval_id = format!("apr_{call_id}");
        let receiver = self.control.register(&approval_id);
        self.events.emit(EventKind::ApprovalRequested {
            approval_id: approval_id.clone(),
            call_id: call_id.to_string(),
            summary: format!("{} {} — {reason}", request.name(), describe(request)),
            options: Approval::ALL
                .iter()
                .map(|choice| choice.as_str().to_string())
                .collect(),
        });
        let (choice, by) = loop {
            match receiver.recv_timeout(self.poll) {
                Ok(choice) => break (choice, "user"),
                Err(RecvTimeoutError::Timeout) if !self.control.is_cancelled() => {}
                Err(RecvTimeoutError::Timeout) => {
                    self.control.unregister(&approval_id);
                    break (Approval::Deny, "cancel");
                }
                Err(RecvTimeoutError::Disconnected) => break (Approval::Deny, "system"),
            }
        };
        self.events.emit(EventKind::ApprovalResolved {
            approval_id,
            choice: choice.as_str().to_string(),
            by: by.to_string(),
        });
        choice
    }
}

/// Everything needed to run one task on its own thread.
pub struct TaskLaunch {
    /// Metadata database file. The task row (`Store::create_task`) must
    /// already exist.
    pub db_path: PathBuf,
    pub spec: TaskSpec,
    pub mode: Mode,
    pub context: Context,
    pub executor: Box<dyn AgentExecutor + Send>,
    pub tools: Box<dyn ToolExecutor + Send>,
}

/// Optional extras for [`spawn_task_with`]. Kept apart from [`TaskLaunch`]
/// so existing launch sites keep compiling.
#[derive(Default)]
pub struct TaskOptions {
    /// Capture the work tree before each mutating step (per-step undo, see
    /// [`crate::snapshot`]).
    pub snapshots: Option<Box<dyn StepSnapshots + Send>>,
}

/// A running task. Dropping the handle does not cancel the task.
#[derive(Debug)]
pub struct TaskHandle {
    task_id: TaskId,
    control: Arc<TaskControl>,
    events: Receiver<AgentEvent>,
    join: JoinHandle<Result<TaskStatus, String>>,
}

impl TaskHandle {
    #[must_use]
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    /// Ordered, already-persisted events. Ends when the task finishes.
    #[must_use]
    pub fn events(&self) -> &Receiver<AgentEvent> {
        &self.events
    }

    /// Shared control block (cancel / respond from other threads).
    #[must_use]
    pub fn control(&self) -> Arc<TaskControl> {
        Arc::clone(&self.control)
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// Answer an `approval_requested` event.
    ///
    /// # Errors
    /// See [`TaskControl::respond`].
    pub fn respond(&self, approval_id: &str, approval: Approval) -> Result<(), ControlError> {
        self.control.respond(approval_id, approval)
    }

    /// Wait for the task thread.
    ///
    /// # Errors
    /// Returns a message if the database could not be opened or the task
    /// thread panicked.
    pub fn join(self) -> Result<TaskStatus, String> {
        self.join
            .join()
            .map_err(|_| "task thread panicked".to_string())?
    }
}

/// Start a task on a dedicated thread: open the store, persist every event
/// before forwarding it, route tool calls through the permission engine, and
/// update the task row's status at the end.
///
/// The thread is a plain OS thread so the synchronous broker and blocking
/// tool execution never stall an async runtime.
#[must_use]
pub fn spawn_task(launch: TaskLaunch) -> TaskHandle {
    spawn_task_with(launch, TaskOptions::default())
}

/// [`spawn_task`] with [`TaskOptions`] (e.g. per-step snapshots).
#[must_use]
pub fn spawn_task_with(launch: TaskLaunch, options: TaskOptions) -> TaskHandle {
    let control = TaskControl::new();
    let (sender, events) = mpsc::channel();
    let task_id = launch.spec.id.clone();
    let thread_control = Arc::clone(&control);
    let join = std::thread::Builder::new()
        .name(format!("task-{task_id}"))
        .spawn(move || run_launch(launch, options, &thread_control, sender))
        .unwrap_or_else(|error| {
            // Spawning only fails on resource exhaustion; surface it through
            // `join` like any other startup failure.
            let message = format!("could not start task thread: {error}");
            std::thread::spawn(move || Err(message))
        });
    TaskHandle {
        task_id,
        control,
        events,
        join,
    }
}

fn run_launch(
    launch: TaskLaunch,
    options: TaskOptions,
    control: &Arc<TaskControl>,
    forward: Sender<AgentEvent>,
) -> Result<TaskStatus, String> {
    let TaskLaunch {
        db_path,
        spec,
        mode,
        context,
        mut executor,
        tools,
    } = launch;
    let store = Store::open(&db_path).map_err(|error| error.to_string())?;
    let sink_store = Store::open(&db_path).map_err(|error| error.to_string())?;
    let task_id = spec.id.to_string();
    let _ = store.set_task_status(&task_id, TaskStatus::Running, Some(executor.id()));

    let events = Emitter::new(spec.id.clone())
        .with_sink(StoreSink::new(sink_store))
        .with_sink(forward);
    let mut broker = SqliteBroker::new(&store, mode, context)
        .for_task(task_id.clone())
        .with_boxed_executor(tools)
        .with_approver(ChannelApprover::new(Arc::clone(control), events.clone()));
    if let Some(snapshots) = options.snapshots {
        broker = broker.with_snapshots(snapshots);
    }
    broker.load_rules().map_err(|error| error.to_string())?;

    let cancelled = || control.is_cancelled();
    let ctx = RunContext {
        events: &events,
        cancelled: &cancelled,
    };
    let status = run_task_with(executor.as_mut(), &mut broker, &spec, &ctx);
    let _ = store.set_task_status(&task_id, status, None);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Collector;

    #[test]
    fn respond_to_unknown_approval_fails() {
        let control = TaskControl::new();
        assert_eq!(
            control.respond("apr_x", Approval::Allow),
            Err(ControlError::UnknownApproval("apr_x".into()))
        );
    }

    #[test]
    fn approver_round_trip_emits_requested_then_resolved() {
        let control = TaskControl::new();
        let collector = Collector::new();
        let events = Emitter::new(TaskId::from_seed("ap")).with_sink(collector.clone());
        let mut approver = ChannelApprover::new(Arc::clone(&control), events);

        let responder = {
            let control = Arc::clone(&control);
            std::thread::spawn(move || loop {
                if control.pending_approvals() == ["apr_c1"] {
                    control
                        .respond("apr_c1", Approval::AlwaysForWorkspace)
                        .expect("respond");
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            })
        };
        let request = ToolRequest::Write {
            path: "/w/a".into(),
            contents: "x".into(),
        };
        assert_eq!(
            approver.approve("c1", &request, "manual mode"),
            Approval::AlwaysForWorkspace
        );
        responder.join().expect("responder");

        let kinds: Vec<&str> = collector.events().iter().map(|e| e.kind.kind()).collect();
        assert_eq!(kinds, ["approval_requested", "approval_resolved"]);
        let resolved = collector.events()[1].data();
        assert_eq!(resolved["choice"], "always_for_workspace");
        assert_eq!(resolved["by"], "user");
        assert_eq!(control.pending_approvals(), Vec::<String>::new());
    }

    #[test]
    fn cancel_while_waiting_denies() {
        let control = TaskControl::new();
        let collector = Collector::new();
        let events = Emitter::new(TaskId::from_seed("ap2")).with_sink(collector.clone());
        let mut approver = ChannelApprover::new(Arc::clone(&control), events);
        control.cancel();
        let request = ToolRequest::Exec {
            command: "make".into(),
            cwd: "/w".into(),
        };
        assert_eq!(approver.approve("c9", &request, "r"), Approval::Deny);
        assert_eq!(collector.events()[1].data()["by"], "cancel");
        assert_eq!(control.pending_approvals(), Vec::<String>::new());
    }

    #[test]
    fn approval_labels_round_trip() {
        for choice in Approval::ALL {
            assert_eq!(Approval::parse(choice.as_str()), Some(choice));
        }
        assert_eq!(Approval::parse("maybe"), None);
    }
}
