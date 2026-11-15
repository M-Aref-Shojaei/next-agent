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

use std::fs;
use std::io::Write as _;
use std::path::Path;

/// Write through a temp file in the same folder, fsync, then rename, so a
/// crash never leaves a half-written file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let temp = dir.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
