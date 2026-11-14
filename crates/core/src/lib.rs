#![forbid(unsafe_code)]

//! Domain models and workspace management for Next Agent.

pub mod agent;
pub mod broker;
pub mod builtin;
pub mod compaction;
pub mod control;
pub mod edit_match;
pub mod events;
pub mod hash;
pub mod llm;
pub mod local_tools;
pub mod profiles;
pub mod router;
pub mod runner;
pub mod snapshot;
pub mod store;
pub mod subagent;
pub mod task;
pub mod tools;
pub mod vault;
pub mod workspace;
pub mod worktree;

pub use agent::{AgentExecutor, AgentStep, MockExecutor, TaskSpec};
pub use broker::{
    context_for, row_to_rule, to_policy_request, Approval, Approver, SqliteBroker, ToolExecutor,
    UnavailableExecutor,
};
pub use builtin::BuiltinExecutor;
pub use control::{
    spawn_task, spawn_task_with, ChannelApprover, TaskControl, TaskHandle, TaskLaunch, TaskOptions,
};
pub use events::{AgentEvent, Emitter, EventKind, EventLog, EventSink, StoreSink};
pub use local_tools::LocalTools;
pub use runner::{run_task, run_task_with, BrokerOutcome, Floor, RunContext, ToolBroker};
pub use snapshot::{
    undo_last_step, undoable_steps, GitSnapshots, StepSnapshots, UndoError, UndoOutcome,
};
pub use store::{
    AuditEntry, AuditVerification, RuleRow, StepSnapshotRow, Store, StoreError, StoredWorkspace,
};
pub use task::{TaskId, TaskStatus};
pub use tools::{ToolRequest, ToolResult};
pub use workspace::{Workspace, WorkspaceError};
pub use worktree::{
    GitCli, MergeOptions, MergeOutcome, MergeStrategy, ShadowRepo, StructuredDiff, TaskWorktree,
    WorktreeError, WorktreeProvider,
};
