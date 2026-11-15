//! Memory and learning (ported from Hermes Agent, MIT © 2025 Nous Research;
//! see `docs/upstream/Hermes Agent.md`): curated memory, SKILL.md skills,
//! the session store and its search tool, the background review that runs
//! after a task, and cron jobs.
//!
//! Every tool here reaches the disk only as a [`ToolRequest::Learn`]
//! through the broker and the permission engine; [`LearningTools`] is the
//! executor the broker hands allowed calls to.
//!
//! [`ToolRequest::Learn`]: crate::tools::ToolRequest::Learn

pub mod cron;
pub mod memory;
pub mod review;
pub mod sessions;
pub mod skills;
pub mod threats;
pub mod tools;
pub mod yaml;

pub use tools::{LearnChange, Learning, LearningTools, ToolSet};

use serde_json::{json, Value};

/// The JSON a learning tool returns on failure.
pub(crate) fn fail(message: impl Into<String>) -> Value {
    json!({"success": false, "error": message.into()})
}
