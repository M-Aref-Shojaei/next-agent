#![forbid(unsafe_code)]

//! Domain models and workspace management for Next Agent.

pub mod agent;
pub mod broker;
pub mod builtin;
pub mod control;
pub mod events;
pub mod hash;
pub mod llm;
pub mod local_tools;
pub mod router;
pub mod runner;
pub mod store;
pub mod task;
pub mod tools;
pub mod workspace;
pub mod worktree;

pub use agent::{AgentExecutor, AgentStep, MockExecutor, TaskSpec};
pub use broker::{
    context_for, row_to_rule, to_policy_request, Approval, Approver, SqliteBroker, ToolExecutor,
    UnavailableExecutor,
};
pub use builtin::BuiltinExecutor;
pub use control::{spawn_task, ChannelApprover, TaskControl, TaskHandle, TaskLaunch};
pub use events::{AgentEvent, Emitter, EventKind, EventLog, EventSink, StoreSink};
pub use local_tools::LocalTools;
pub use runner::{run_task, run_task_with, BrokerOutcome, RunContext, ToolBroker};
pub use store::{AuditEntry, AuditVerification, RuleRow, Store, StoreError, StoredWorkspace};
pub use task::{TaskId, TaskStatus};
pub use tools::{ToolRequest, ToolResult};
pub use workspace::{Workspace, WorkspaceError};
pub use worktree::{
    GitCli, MergeOptions, MergeOutcome, MergeStrategy, StructuredDiff, TaskWorktree, WorktreeError,
    WorktreeProvider,
};
