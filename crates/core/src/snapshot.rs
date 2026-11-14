// Design adapted from opencode (MIT, © 2025 opencode): packages/core/src/snapshot.ts @ cbe71595e1f370ff91b3081872b96bbe56519c41
//! Per-step undo.
//!
//! Before the broker executes a mutating tool call (write, patch, edit,
//! command) it asks a [`StepSnapshots`] for a capture of the work tree and
//! records it in `agent_step_snapshots`. [`undo_last_step`] restores the work
//! tree to the newest capture that has not been undone yet, so repeated
//! calls walk back one step at a time.
//!
//! The git work lives in [`ShadowRepo`] (`worktree.rs`, the only module that
//! may spawn git): a private `GIT_DIR` under the app data directory, keyed by
//! the work tree, so the user's repository, index and history are never
//! touched.
//!
//! Undo is a user action (a desktop command), not an agent tool, so it does
//! not pass through the permission engine; it can only restore paths inside
//! the recorded work tree.

use crate::store::{StepSnapshotRow, Store};
use crate::worktree::{ShadowRepo, WorktreeError};
use std::path::{Path, PathBuf};

/// A capture taken before one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStep {
    pub work_tree: String,
    pub git_dir: String,
    pub tree: String,
}

/// Captures the work tree before a mutating step. Best effort: a failed
/// capture never blocks the step.
pub trait StepSnapshots {
    /// # Errors
    /// A human-readable reason when the capture failed.
    fn capture(&mut self) -> Result<CapturedStep, String>;
}

/// Shadow-git implementation of [`StepSnapshots`].
#[derive(Debug, Clone)]
pub struct GitSnapshots {
    repo: ShadowRepo,
}

impl GitSnapshots {
    /// Snapshots of `work_tree` (the task's sandbox root: the worktree in
    /// `auto` mode, the workspace otherwise) for `task_id`, stored under
    /// `data_dir`. Each task gets its own shadow repository, so concurrent
    /// tasks never contend for one index. (Undo still restores the shared
    /// work tree, so undoing one task can revert another task's later edits
    /// to the same files; the UI should warn when tasks overlap.)
    ///
    /// # Errors
    /// Git failures, or a missing work tree.
    pub fn open(data_dir: &Path, work_tree: &Path, task_id: &str) -> Result<Self, WorktreeError> {
        Ok(Self {
            repo: ShadowRepo::open(data_dir, work_tree, task_id)?,
        })
    }
}

impl StepSnapshots for GitSnapshots {
    fn capture(&mut self) -> Result<CapturedStep, String> {
        let tree = self.repo.capture().map_err(|error| error.to_string())?;
        Ok(CapturedStep {
            work_tree: self.repo.work_tree.to_string_lossy().into_owned(),
            git_dir: self.repo.git_dir.to_string_lossy().into_owned(),
            tree,
        })
    }
}

/// What [`undo_last_step`] restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    /// The snapshot's sequence within the task.
    pub seq: i64,
    /// The tool call whose effects were undone.
    pub call_id: String,
    pub tool: String,
    /// Work-tree-relative paths that changed back.
    pub restored: Vec<String>,
    /// Steps still undoable after this one.
    pub remaining: usize,
}

/// Why nothing was undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoError {
    NothingToUndo,
    Store(String),
    Git(String),
}

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToUndo => f.write_str("there is no step left to undo for this task"),
            Self::Store(message) => write!(f, "could not read snapshots: {message}"),
            Self::Git(message) => write!(f, "could not restore the snapshot: {message}"),
        }
    }
}

impl std::error::Error for UndoError {}

/// Steps of `task_id` that can still be undone, newest last.
///
/// # Errors
/// [`UndoError::Store`] if the snapshot table cannot be read.
pub fn undoable_steps(store: &Store, task_id: &str) -> Result<Vec<StepSnapshotRow>, UndoError> {
    Ok(store
        .step_snapshots(task_id)
        .map_err(|error| UndoError::Store(error.to_string()))?
        .into_iter()
        .filter(|row| row.undone_at.is_none())
        .collect())
}

/// Undo the newest not-yet-undone step of `task_id`: restore its work tree to
/// the capture taken right before that step ran. Call only while the task is
/// not running.
///
/// # Errors
/// [`UndoError::NothingToUndo`] when every step is undone (or none was
/// recorded), otherwise store or git failures.
pub fn undo_last_step(store: &Store, task_id: &str) -> Result<UndoOutcome, UndoError> {
    let mut steps = undoable_steps(store, task_id)?;
    let last = steps.pop().ok_or(UndoError::NothingToUndo)?;
    let repo = ShadowRepo::from_parts(PathBuf::from(&last.git_dir), PathBuf::from(&last.work_tree));
    let restored = repo
        .restore_all(&last.tree)
        .map_err(|error| UndoError::Git(error.to_string()))?;
    store
        .mark_step_undone(task_id, last.seq)
        .map_err(|error| UndoError::Store(error.to_string()))?;
    Ok(UndoOutcome {
        seq: last.seq,
        call_id: last.call_id,
        tool: last.tool,
        restored,
        remaining: steps.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::{context_for, SqliteBroker};
    use crate::local_tools::LocalTools;
    use crate::runner::ToolBroker;
    use crate::tools::ToolRequest;
    use next_agent_permission_engine::Mode;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir =
            std::env::temp_dir().join(format!("na-undo-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        dir.canonicalize().expect("canonical")
    }

    #[test]
    fn undo_walks_back_one_step_at_a_time() {
        let work = temp("work");
        let data = temp("data");
        std::fs::write(work.join("a.txt"), "one\n").expect("seed");
        let store = Store::open_in_memory().expect("store");
        {
            let mut broker = SqliteBroker::new(
                &store,
                Mode::BypassPermissions,
                context_for(work.clone(), "ws".into(), None),
            )
            .with_executor(LocalTools::new())
            .with_snapshots(Box::new(
                GitSnapshots::open(&data, &work, "t1").expect("open"),
            ))
            .for_task("t1");
            let edit = ToolRequest::Edit {
                path: work.join("a.txt"),
                old_string: "one".into(),
                new_string: "two".into(),
                replace_all: false,
            };
            assert!(broker.handle("c1", &edit).result.ok);
            let create = ToolRequest::Write {
                path: work.join("sub/new.txt"),
                contents: "new\n".into(),
            };
            assert!(broker.handle("c2", &create).result.ok);
            // Reads are not steps.
            let read = ToolRequest::Read {
                path: work.join("a.txt"),
            };
            assert!(broker.handle("c3", &read).result.ok);
        }
        assert_eq!(undoable_steps(&store, "t1").expect("steps").len(), 2);

        let second = undo_last_step(&store, "t1").expect("undo c2");
        assert_eq!(second.call_id, "c2");
        assert_eq!(second.restored, ["sub/new.txt"]);
        assert_eq!(second.remaining, 1);
        assert!(!work.join("sub").exists());
        assert_eq!(
            std::fs::read_to_string(work.join("a.txt")).expect("a"),
            "two\n"
        );

        let first = undo_last_step(&store, "t1").expect("undo c1");
        assert_eq!(
            (first.call_id.as_str(), first.tool.as_str()),
            ("c1", "edit")
        );
        assert_eq!(
            std::fs::read_to_string(work.join("a.txt")).expect("a"),
            "one\n"
        );
        assert_eq!(undo_last_step(&store, "t1"), Err(UndoError::NothingToUndo));
        // The work tree never became a git repository; snapshots live in data.
        assert!(!work.join(".git").exists());
        let _ = std::fs::remove_dir_all(&work);
        let _ = std::fs::remove_dir_all(&data);
    }
}
