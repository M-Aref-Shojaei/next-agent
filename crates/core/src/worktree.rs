//! Git worktree isolation for `auto`-mode tasks (M1 step 4).
//!
//! Seam: [`WorktreeProvider`] (see `docs/architecture/Module Boundaries.md`).
//! The only implementation, [`GitCli`], shells out to the system `git`; **this file is the
//! only place allowed to spawn `git`** (enforced by `scripts/check-boundaries.mjs`).
//!
//! Lifecycle: [`ensure_task_worktree`](WorktreeProvider::ensure_task_worktree) →
//! (agent edits inside [`TaskWorktree::root`]) → [`checkpoint`](WorktreeProvider::checkpoint) →
//! [`diff`](WorktreeProvider::diff) → [`merge`](WorktreeProvider::merge) /
//! [`discard`](WorktreeProvider::discard) / [`keep`](WorktreeProvider::keep).
//!
//! Nothing here ever writes to the main working tree until [`merge`](WorktreeProvider::merge).
//!
//! [`ShadowRepo`] (per-step undo) also lives here because it spawns git: it keeps snapshots in a
//! private `GIT_DIR` under the app data directory and never touches the user's repository.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::task::TaskId;

const BRANCH_PREFIX: &str = "next-agent/auto-";
const WORKTREE_DIR: &str = ".next-agent/worktrees";
const EXCLUDE_ENTRY: &str = ".next-agent/";

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// An isolated checkout for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWorktree {
    pub task_id: TaskId,
    /// Canonical root of the main repository.
    pub repo_root: PathBuf,
    /// The task's sandbox root / default cwd.
    pub root: PathBuf,
    /// `next-agent/auto-<short-id>`.
    pub branch: String,
    /// HEAD commit of the main tree when the worktree was created.
    pub base_commit: String,
}

/// How a task branch is folded into the branch checked out in the main tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeStrategy {
    /// One commit on the target branch (default, resolves Q5: agent checkpoint noise stays out
    /// of the user's history).
    #[default]
    Squash,
    /// `--no-ff` merge commit, preserving the agent's checkpoint commits.
    NoFastForward,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeOptions {
    pub strategy: MergeStrategy,
    /// Commit message; defaults to `next-agent: task <id>`.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    /// New commit on the target branch; `None` when the task changed nothing.
    pub commit: Option<String>,
    pub strategy: MergeStrategy,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Text after the second `@@` (usually the enclosing function).
    pub section: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// Set for renames.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
}

/// Structured `<base>...<branch>` diff for the review UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructuredDiff {
    pub files: Vec<FileDiff>,
}

impl StructuredDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Typed failures. Callers fall back to `manual` mode when
/// [`WorktreeError::should_fall_back_to_manual`] is true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeError {
    /// Workspace is not (the root of) a git repository (Q4: refuse, fall back to manual).
    NotAGitRepo(PathBuf),
    /// The `git` binary could not be run.
    GitUnavailable(String),
    /// Repository has no commits yet, so there is no HEAD to branch from.
    NoCommits,
    /// Main tree is on a detached HEAD; there is no branch to merge into.
    DetachedHead,
    /// Main tree has staged changes, or uncommitted changes in files the task also changed.
    DirtyMainTree { files: Vec<String> },
    /// Merge conflicted; the main tree has been restored to its pre-merge state.
    MergeConflict { files: Vec<String> },
    /// No worktree exists for this task.
    NotFound(TaskId),
    /// Caller-supplied worktree metadata does not match repository state.
    InvalidIdentity { task_id: TaskId, reason: String },
    /// Any other failing git invocation.
    Git { command: String, stderr: String },
}

impl WorktreeError {
    /// A stable label for logs: the variant, never paths or git output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotAGitRepo(_) => "not_a_git_repo",
            Self::GitUnavailable(_) => "git_unavailable",
            Self::NoCommits => "no_commits",
            Self::DetachedHead => "detached_head",
            Self::DirtyMainTree { .. } => "dirty_main_tree",
            Self::MergeConflict { .. } => "merge_conflict",
            Self::NotFound(_) => "not_found",
            Self::InvalidIdentity { .. } => "invalid_identity",
            Self::Git { .. } => "git",
        }
    }

    #[must_use]
    pub fn should_fall_back_to_manual(&self) -> bool {
        matches!(
            self,
            Self::NotAGitRepo(_) | Self::GitUnavailable(_) | Self::NoCommits
        )
    }
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAGitRepo(p) => write!(f, "not a git repository root: {}", p.display()),
            Self::GitUnavailable(e) => write!(f, "git is unavailable: {e}"),
            Self::NoCommits => write!(f, "repository has no commits"),
            Self::DetachedHead => write!(f, "main working tree is on a detached HEAD"),
            Self::DirtyMainTree { files } => {
                write!(
                    f,
                    "main working tree has conflicting changes: {}",
                    files.join(", ")
                )
            }
            Self::MergeConflict { files } => write!(f, "merge conflict in: {}", files.join(", ")),
            Self::NotFound(id) => write!(f, "no worktree for task {id}"),
            Self::InvalidIdentity { task_id, reason } => {
                write!(f, "invalid worktree identity for task {task_id}: {reason}")
            }
            Self::Git { command, stderr } => write!(f, "git {command} failed: {}", stderr.trim()),
        }
    }
}

impl std::error::Error for WorktreeError {}

type Result<T> = std::result::Result<T, WorktreeError>;

// ---------------------------------------------------------------------------------------------
// Seam
// ---------------------------------------------------------------------------------------------

/// Git isolation seam. Implementations: [`GitCli`].
pub trait WorktreeProvider {
    /// Create (or return the existing) worktree for `task_id` from the main tree's HEAD.
    ///
    /// # Errors
    /// [`WorktreeError::NotAGitRepo`] / [`WorktreeError::NoCommits`] for workspaces where auto
    /// mode is unavailable; other variants for git failures.
    fn ensure_task_worktree(&self, repo_root: &Path, task_id: &TaskId) -> Result<TaskWorktree>;

    /// Look up an existing worktree without creating one.
    ///
    /// # Errors
    /// Git failures.
    fn find(&self, repo_root: &Path, task_id: &TaskId) -> Result<Option<TaskWorktree>>;

    /// Commit everything in the worktree to the task branch. Returns the new commit, or `None`
    /// if there was nothing to commit.
    ///
    /// # Errors
    /// Git failures.
    fn checkpoint(&self, wt: &TaskWorktree, message: &str) -> Result<Option<String>>;

    /// Structured diff of the *committed* task branch against its base (`base...branch`).
    /// Call [`checkpoint`](Self::checkpoint) first to include uncommitted edits.
    ///
    /// # Errors
    /// Git failures.
    fn diff(&self, wt: &TaskWorktree) -> Result<StructuredDiff>;

    /// Checkpoint, merge into the branch checked out in the main tree, then remove the worktree
    /// and branch. On error nothing is lost and the main tree is left as it was.
    ///
    /// # Errors
    /// [`WorktreeError::DirtyMainTree`], [`WorktreeError::MergeConflict`],
    /// [`WorktreeError::DetachedHead`], or git failures.
    fn merge(&self, wt: &TaskWorktree, options: &MergeOptions) -> Result<MergeOutcome>;

    /// Remove the worktree and delete the branch. Idempotent.
    ///
    /// # Errors
    /// Git failures.
    fn discard(&self, wt: &TaskWorktree) -> Result<()>;

    /// Checkpoint and leave worktree + branch in place for later review.
    ///
    /// # Errors
    /// Git failures.
    fn keep(&self, wt: &TaskWorktree) -> Result<()>;

    /// All task worktrees (active or kept) for the sidebar / settings list.
    ///
    /// # Errors
    /// Git failures.
    fn list(&self, repo_root: &Path) -> Result<Vec<TaskWorktree>>;

    /// `git worktree prune` (run on startup to drop records of deleted directories).
    ///
    /// # Errors
    /// Git failures.
    fn prune(&self, repo_root: &Path) -> Result<()>;
}

// ---------------------------------------------------------------------------------------------
// git CLI adapter (the only code that spawns git)
// ---------------------------------------------------------------------------------------------

/// Shells out to the system `git`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitCli;

struct GitOutput {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run_git(dir: &Path, args: &[&str]) -> Result<GitOutput> {
    let out = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|e| WorktreeError::GitUnavailable(e.to_string()))?;
    Ok(GitOutput {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run_git(dir, args)?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(WorktreeError::Git {
            command: args.join(" "),
            stderr: out.stderr,
        })
    }
}

fn short_id(task_id: &TaskId) -> String {
    let id = task_id.as_str();
    id.strip_prefix("tsk_")
        .unwrap_or(id)
        .chars()
        .take(8)
        .collect()
}

fn branch_name(task_id: &TaskId) -> String {
    format!("{BRANCH_PREFIX}{}", short_id(task_id))
}

fn nul_list(s: &str) -> Vec<String> {
    s.split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

fn canonical_path(path: &Path) -> std::io::Result<PathBuf> {
    let path = path.canonicalize()?;
    #[cfg(windows)]
    if let Some(path) = path.to_str().and_then(|path| path.strip_prefix(r"\\?\")) {
        return Ok(PathBuf::from(path));
    }
    Ok(path)
}

impl GitCli {
    /// Validate `repo_root` is the toplevel of a repo with at least one commit.
    fn open_repo(repo_root: &Path) -> Result<PathBuf> {
        let canonical = canonical_path(repo_root)
            .map_err(|_| WorktreeError::NotAGitRepo(repo_root.to_path_buf()))?;
        let out = run_git(&canonical, &["rev-parse", "--show-toplevel"])?;
        if !out.ok {
            return Err(WorktreeError::NotAGitRepo(canonical));
        }
        let top = PathBuf::from(out.stdout.trim());
        if top.canonicalize().ok().as_deref() != Some(canonical.as_path()) {
            return Err(WorktreeError::NotAGitRepo(canonical));
        }
        Ok(canonical)
    }

    fn head(repo: &Path) -> Result<String> {
        let out = run_git(repo, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
        if out.ok {
            Ok(out.stdout.trim().to_owned())
        } else {
            Err(WorktreeError::NoCommits)
        }
    }

    fn ensure_excluded(repo: &Path) -> Result<()> {
        let raw = git(repo, &["rev-parse", "--git-path", "info/exclude"])?;
        let mut path = PathBuf::from(raw.trim());
        if path.is_relative() {
            path = repo.join(path);
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == EXCLUDE_ENTRY) {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut next = existing;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(EXCLUDE_ENTRY);
        next.push('\n');
        std::fs::write(&path, next).map_err(|e| WorktreeError::Git {
            command: "write info/exclude".into(),
            stderr: e.to_string(),
        })
    }

    fn base_of(repo: &Path, branch: &str) -> Result<String> {
        let key = format!("branch.{branch}.nextAgentBase");
        let out = run_git(repo, &["config", "--get", &key])?;
        if out.ok && !out.stdout.trim().is_empty() {
            return Ok(out.stdout.trim().to_owned());
        }
        Ok(git(repo, &["merge-base", "HEAD", branch])?
            .trim()
            .to_owned())
    }

    fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
        let r = format!("refs/heads/{branch}");
        Ok(run_git(repo, &["rev-parse", "--verify", "--quiet", &r])?.ok)
    }

    /// Paths with uncommitted changes in the main tree, and whether anything is staged.
    fn main_tree_status(repo: &Path) -> Result<(Vec<String>, bool)> {
        let raw = git(
            repo,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?;
        let mut parts = raw.split('\0').filter(|p| !p.is_empty());
        let (mut paths, mut staged) = (Vec::new(), false);
        while let Some(entry) = parts.next() {
            if entry.len() < 4 {
                continue;
            }
            let (x, y) = (entry.as_bytes()[0], entry.as_bytes()[1]);
            if x != b' ' && x != b'?' && x != b'!' {
                staged = true;
            }
            paths.push(entry[3..].to_owned());
            if x == b'R' || x == b'C' || y == b'R' || y == b'C' {
                if let Some(orig) = parts.next() {
                    paths.push(orig.to_owned());
                }
            }
        }
        Ok((paths, staged))
    }

    fn changed_files(repo: &Path, base: &str, branch: &str) -> Result<Vec<String>> {
        let range = format!("{base}..{branch}");
        Ok(nul_list(&git(
            repo,
            &["diff", "--name-only", "-z", "--no-renames", &range],
        )?))
    }

    fn conflicted_files(repo: &Path) -> Vec<String> {
        run_git(repo, &["diff", "--name-only", "-z", "--diff-filter=U"])
            .map_or_else(|_| Vec::new(), |o| nul_list(&o.stdout))
    }

    fn make_wt(repo: &Path, task_id: &TaskId) -> Result<Option<TaskWorktree>> {
        let branch = branch_name(task_id);
        let root = repo.join(WORKTREE_DIR).join(task_id.as_str());
        if !root.is_dir() || !Self::branch_exists(repo, &branch)? {
            return Ok(None);
        }
        Ok(Some(TaskWorktree {
            task_id: task_id.clone(),
            repo_root: repo.to_path_buf(),
            root,
            base_commit: Self::base_of(repo, &branch)?,
            branch,
        }))
    }

    fn validate_identity(wt: &TaskWorktree) -> Result<()> {
        let repo = canonical_path(&wt.repo_root).map_err(|_| WorktreeError::InvalidIdentity {
            task_id: wt.task_id.clone(),
            reason: "repository root is unavailable".into(),
        })?;
        if repo != wt.repo_root
            || wt.branch != branch_name(&wt.task_id)
            || wt.root != repo.join(WORKTREE_DIR).join(wt.task_id.as_str())
            || !wt.root.is_dir()
        {
            return Err(WorktreeError::InvalidIdentity {
                task_id: wt.task_id.clone(),
                reason: "repo, root, task, or branch metadata does not match expected worktree"
                    .into(),
            });
        }
        let actual_branch = git(&wt.root, &["branch", "--show-current"])?;
        if actual_branch.trim() != wt.branch {
            return Err(WorktreeError::InvalidIdentity {
                task_id: wt.task_id.clone(),
                reason: "worktree is not checked out on recorded branch".into(),
            });
        }
        let recorded_base = Self::base_of(&repo, &wt.branch)?;
        if recorded_base != wt.base_commit
            || !run_git(
                &repo,
                &["cat-file", "-e", &format!("{}^{{commit}}", wt.base_commit)],
            )?
            .ok
        {
            return Err(WorktreeError::InvalidIdentity {
                task_id: wt.task_id.clone(),
                reason: "base commit metadata is stale or invalid".into(),
            });
        }
        Ok(())
    }

    fn cleanup(wt: &TaskWorktree) -> Result<()> {
        Self::validate_identity(wt)?;
        let repo = &wt.repo_root;
        if wt.root.exists() {
            let path = wt.root.to_string_lossy().into_owned();
            let out = run_git(repo, &["worktree", "remove", "--force", &path])?;
            if !out.ok && wt.root.exists() {
                return Err(WorktreeError::Git {
                    command: "worktree remove".into(),
                    stderr: out.stderr,
                });
            }
        }
        git(repo, &["worktree", "prune"])?;
        if Self::branch_exists(repo, &wt.branch)? {
            git(repo, &["branch", "-D", &wt.branch])?;
        }
        Ok(())
    }
}

impl GitCli {
    fn merge_branch(wt: &TaskWorktree, options: &MergeOptions) -> Result<MergeOutcome> {
        let repo = &wt.repo_root;
        Self.checkpoint(wt, &format!("next-agent: checkpoint {}", wt.task_id))?;

        let target = git(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .map_err(|_| WorktreeError::DetachedHead)?;
        let _ = target;

        let files = Self::changed_files(repo, &wt.base_commit, &wt.branch)?;
        if files.is_empty() {
            Self::cleanup(wt)?;
            return Ok(MergeOutcome {
                commit: None,
                strategy: options.strategy,
                files_changed: files,
            });
        }

        let (dirty, staged) = Self::main_tree_status(repo)?;
        let mut blocking: Vec<String> = dirty.into_iter().filter(|p| files.contains(p)).collect();
        if staged {
            // Squash/merge commits would sweep staged user changes in; refuse.
            let staged_paths = nul_list(&git(repo, &["diff", "--cached", "--name-only", "-z"])?);
            for p in staged_paths {
                if !blocking.contains(&p) {
                    blocking.push(p);
                }
            }
        }
        if !blocking.is_empty() {
            blocking.sort();
            return Err(WorktreeError::DirtyMainTree { files: blocking });
        }

        let before = Self::head(repo)?;
        let message = options
            .message
            .clone()
            .unwrap_or_else(|| format!("next-agent: task {}", wt.task_id));

        let result = match options.strategy {
            MergeStrategy::Squash => run_git(repo, &["merge", "--squash", &wt.branch]),
            MergeStrategy::NoFastForward => {
                run_git(repo, &["merge", "--no-ff", "-m", &message, &wt.branch])
            }
        }?;
        if !result.ok {
            let mut conflicts = Self::conflicted_files(repo);
            conflicts.sort();
            // Restore the main tree (we verified it had no conflicting local edits).
            match options.strategy {
                MergeStrategy::Squash => {
                    let _ = run_git(repo, &["reset", "--merge"]);
                }
                MergeStrategy::NoFastForward => {
                    let _ = run_git(repo, &["merge", "--abort"]);
                }
            }
            if conflicts.is_empty() {
                return Err(WorktreeError::Git {
                    command: "merge".into(),
                    stderr: format!("{}{}", result.stdout, result.stderr),
                });
            }
            return Err(WorktreeError::MergeConflict { files: conflicts });
        }
        if options.strategy == MergeStrategy::Squash
            && !run_git(repo, &["diff", "--cached", "--quiet"])?.ok
        {
            git(repo, &["commit", "-m", &message])?;
        }

        let after = Self::head(repo)?;
        Self::cleanup(wt)?;
        Ok(MergeOutcome {
            commit: (after != before).then_some(after),
            strategy: options.strategy,
            files_changed: files,
        })
    }
}

impl WorktreeProvider for GitCli {
    fn ensure_task_worktree(&self, repo_root: &Path, task_id: &TaskId) -> Result<TaskWorktree> {
        let repo = Self::open_repo(repo_root)?;
        if let Some(existing) = Self::make_wt(&repo, task_id)? {
            return Ok(existing);
        }
        let base = Self::head(&repo)?;
        Self::ensure_excluded(&repo)?;
        let branch = branch_name(task_id);
        let root = repo.join(WORKTREE_DIR).join(task_id.as_str());
        let root_str = root.to_string_lossy().into_owned();
        git(&repo, &["worktree", "add", &root_str, "-b", &branch, &base])?;
        git(
            &repo,
            &["config", &format!("branch.{branch}.nextAgentBase"), &base],
        )?;
        tracing::info!(target: "worktree", task_id = %task_id, "worktree_created");
        Ok(TaskWorktree {
            task_id: task_id.clone(),
            repo_root: repo,
            root,
            branch,
            base_commit: base,
        })
    }

    fn find(&self, repo_root: &Path, task_id: &TaskId) -> Result<Option<TaskWorktree>> {
        let repo = Self::open_repo(repo_root)?;
        Self::make_wt(&repo, task_id)
    }

    fn checkpoint(&self, wt: &TaskWorktree, message: &str) -> Result<Option<String>> {
        Self::validate_identity(wt)?;
        git(&wt.root, &["add", "-A"])?;
        if run_git(&wt.root, &["diff", "--cached", "--quiet"])?.ok {
            return Ok(None);
        }
        // Agent-authored commit: fixed identity so it works without user git config.
        git(
            &wt.root,
            &[
                "-c",
                "user.name=Next Agent",
                "-c",
                "user.email=agent@next-agent.local",
                "commit",
                "--no-verify",
                "--no-gpg-sign",
                "-m",
                message,
            ],
        )?;
        let commit = git(&wt.root, &["rev-parse", "HEAD"])?.trim().to_owned();
        tracing::info!(target: "worktree", task_id = %wt.task_id, "checkpoint");
        Ok(Some(commit))
    }

    fn diff(&self, wt: &TaskWorktree) -> Result<StructuredDiff> {
        Self::validate_identity(wt)?;
        let range = format!("{}...{}", wt.base_commit, wt.branch);
        let raw = git(
            &wt.repo_root,
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "-M",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "-U3",
                &range,
            ],
        )?;
        Ok(parse_unified_diff(&raw))
    }

    fn merge(&self, wt: &TaskWorktree, options: &MergeOptions) -> Result<MergeOutcome> {
        Self::validate_identity(wt)?;
        let result = Self::merge_branch(wt, options);
        match &result {
            Ok(outcome) => tracing::info!(
                target: "worktree",
                task_id = %wt.task_id,
                files = outcome.files_changed.len(),
                "worktree_merged"
            ),
            Err(error) => tracing::warn!(
                target: "worktree",
                task_id = %wt.task_id,
                kind = error.kind(),
                "worktree_merge_failed"
            ),
        }
        result
    }

    fn discard(&self, wt: &TaskWorktree) -> Result<()> {
        if !wt.root.exists() && !Self::branch_exists(&wt.repo_root, &wt.branch)? {
            return Ok(());
        }
        Self::validate_identity(wt)?;
        Self::cleanup(wt)?;
        tracing::info!(target: "worktree", task_id = %wt.task_id, "worktree_discarded");
        Ok(())
    }
    fn keep(&self, wt: &TaskWorktree) -> Result<()> {
        Self::validate_identity(wt)?;
        self.checkpoint(wt, &format!("next-agent: checkpoint {}", wt.task_id))?;
        Ok(())
    }

    fn list(&self, repo_root: &Path) -> Result<Vec<TaskWorktree>> {
        let repo = Self::open_repo(repo_root)?;
        let raw = git(&repo, &["worktree", "list", "--porcelain"])?;
        let mut out = Vec::new();
        for block in raw.split("\n\n") {
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(p));
                } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(b.to_owned());
                }
            }
            let (Some(path), Some(branch)) = (path, branch) else {
                continue;
            };
            if !branch.starts_with(BRANCH_PREFIX) {
                continue;
            }
            let Some(task_id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| TaskId::parse(n.to_owned()))
            else {
                continue;
            };
            let base_commit = Self::base_of(&repo, &branch)?;
            let candidate = TaskWorktree {
                task_id,
                repo_root: repo.clone(),
                root: path,
                branch,
                base_commit,
            };
            if Self::validate_identity(&candidate).is_err() {
                continue;
            }
            out.push(candidate);
        }
        Ok(out)
    }

    fn prune(&self, repo_root: &Path) -> Result<()> {
        let repo = Self::open_repo(repo_root)?;
        git(&repo, &["worktree", "prune"]).map(|_| ())
    }
}

// ---------------------------------------------------------------------------------------------
// Shadow-git snapshots (per-step undo)
// ---------------------------------------------------------------------------------------------
//
// Design adapted from opencode (MIT, © 2025 opencode): packages/core/src/snapshot.ts @
// cbe71595e1f370ff91b3081872b96bbe56519c41. A private git directory outside the project
// (`<data>/snapshot/<hash(work tree)>`) records content-addressed trees of the work tree, so a
// step can be undone without touching the user's repository, index or history.

/// Untracked files larger than this are left out of snapshots.
pub const SNAPSHOT_MAX_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;
/// `git gc --auto` runs at most this often per shadow repository.
const SNAPSHOT_PACK_INTERVAL_SECS: u64 = 10 * 60;
const SNAPSHOT_PACK_STAMP: &str = "next-agent-packed-at";

/// A shadow repository for one work tree (a workspace root or a task worktree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowRepo {
    /// Private `GIT_DIR` under the app data directory.
    pub git_dir: PathBuf,
    /// Canonical work tree the snapshots describe.
    pub work_tree: PathBuf,
}

impl ShadowRepo {
    /// Where the shadow repository for `work_tree` and `scope` (a task id, so two tasks on
    /// one work tree never share an index or lock) lives under `data_dir`.
    #[must_use]
    pub fn location(data_dir: &Path, work_tree: &Path, scope: &str) -> PathBuf {
        let seed = format!("{}\0{scope}", work_tree.to_string_lossy());
        let key = crate::hash::sha256_hex(seed.as_bytes());
        data_dir.join("snapshot").join(&key[..16])
    }

    /// Open (initialising on first use) the shadow repository for `work_tree` and `scope`.
    ///
    /// # Errors
    /// [`WorktreeError::NotAGitRepo`] if `work_tree` does not exist, or git failures.
    pub fn open(data_dir: &Path, work_tree: &Path, scope: &str) -> Result<Self> {
        let work_tree = canonical_path(work_tree)
            .map_err(|_| WorktreeError::NotAGitRepo(work_tree.to_path_buf()))?;
        let git_dir = Self::location(data_dir, &work_tree, scope);
        let repo = Self { git_dir, work_tree };
        if !repo.git_dir.join("HEAD").exists() {
            std::fs::create_dir_all(&repo.git_dir).map_err(|e| WorktreeError::Git {
                command: "init (create dir)".into(),
                stderr: e.to_string(),
            })?;
            repo.git(&["init", "-q"])?;
            for (key, value) in [
                ("core.autocrlf", "false"),
                ("core.safecrlf", "false"),
                ("core.symlinks", "true"),
                ("gc.auto", "256"),
            ] {
                repo.git(&["config", key, value])?;
            }
        }
        Ok(repo)
    }

    /// Re-open an existing shadow repository from stored paths (no init).
    #[must_use]
    pub fn from_parts(git_dir: PathBuf, work_tree: PathBuf) -> Self {
        Self { git_dir, work_tree }
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let git_dir = self.git_dir.to_string_lossy().into_owned();
        let work_tree = self.work_tree.to_string_lossy().into_owned();
        let mut full: Vec<&str> = vec![
            "-c",
            "core.autocrlf=false",
            "--literal-pathspecs",
            "--git-dir",
            &git_dir,
            "--work-tree",
            &work_tree,
        ];
        full.extend_from_slice(args);
        git(&self.work_tree, &full)
    }

    /// Capture the work tree as a tree id. Respects the project's ignore files
    /// (`.gitignore` and `.git/info/exclude`), always skips `.git` and
    /// `.next-agent/`, and leaves out untracked files over 2 MiB.
    ///
    /// # Errors
    /// Git failures.
    pub fn capture(&self) -> Result<String> {
        let base = self.base_excludes();
        self.write_excludes(&base)?;
        let untracked = self.git(&["ls-files", "--others", "--exclude-standard", "-z"])?;
        let large: Vec<String> = nul_list(&untracked)
            .into_iter()
            .filter(|path| {
                std::fs::symlink_metadata(self.work_tree.join(path))
                    .is_ok_and(|m| m.is_file() && m.len() > SNAPSHOT_MAX_UNTRACKED_BYTES)
            })
            .collect();
        if !large.is_empty() {
            let mut excludes = base;
            for path in &large {
                excludes.push('/');
                excludes.push_str(&escape_exclude(path));
                excludes.push('\n');
            }
            self.write_excludes(&excludes)?;
        }
        self.git(&["add", "-A", "--", "."])?;
        let tree = self.git(&["write-tree"])?.trim().to_owned();
        self.pack_when_due();
        Ok(tree)
    }

    /// Paths (work-tree relative, `/`-separated) that differ between `from` and `to`, or
    /// between `from` and the current work tree when `to` is `None`.
    ///
    /// # Errors
    /// Git failures.
    pub fn files(&self, from: &str, to: Option<&str>) -> Result<Vec<String>> {
        let to = self.target(to)?;
        let out = self.git(&["diff", "--name-only", "--no-renames", "-z", from, &to])?;
        Ok(nul_list(&out))
    }

    /// Structured diff between two trees (or `from` and the current work tree).
    ///
    /// # Errors
    /// Git failures.
    pub fn diff(&self, from: &str, to: Option<&str>) -> Result<StructuredDiff> {
        let to = self.target(to)?;
        let raw = self.git(&[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "-U3",
            from,
            &to,
        ])?;
        Ok(parse_unified_diff(&raw))
    }

    /// `to`, or a fresh capture of the work tree when `None`.
    fn target(&self, to: Option<&str>) -> Result<String> {
        match to {
            Some(tree) => Ok(tree.to_owned()),
            None => self.capture(),
        }
    }

    /// Restore `paths` to their content in `tree`. A path absent from `tree` is removed.
    /// Every path must be relative and stay inside the work tree (no `..`, no absolute
    /// paths, no `.git`, no symlinked parent leading outside); otherwise nothing is restored.
    ///
    /// # Errors
    /// [`WorktreeError::Git`] naming the offending path, or git failures.
    pub fn restore(&self, tree: &str, paths: &[String]) -> Result<()> {
        for path in paths {
            self.check_inside(path)?;
        }
        if paths.is_empty() {
            return Ok(());
        }
        let mut args: Vec<&str> = vec!["ls-tree", "-r", "-z", "--name-only", tree, "--"];
        args.extend(paths.iter().map(String::as_str));
        let present: std::collections::HashSet<String> =
            nul_list(&self.git(&args)?).into_iter().collect();
        let (keep, gone): (Vec<&String>, Vec<&String>) =
            paths.iter().partition(|path| present.contains(*path));
        if !keep.is_empty() {
            let mut args: Vec<&str> = vec!["checkout", tree, "--"];
            args.extend(keep.iter().map(|p| p.as_str()));
            self.git(&args)?;
        }
        for path in gone {
            let full = self.work_tree.join(path);
            match std::fs::symlink_metadata(&full) {
                Ok(meta) if meta.is_dir() => {
                    let _ = std::fs::remove_dir_all(&full);
                }
                Ok(_) => {
                    let _ = std::fs::remove_file(&full);
                }
                Err(_) => {}
            }
            // Drop now-empty parent directories the step created.
            let mut parent = full.parent();
            while let Some(dir) = parent {
                if dir == self.work_tree || std::fs::remove_dir(dir).is_err() {
                    break;
                }
                parent = dir.parent();
            }
        }
        Ok(())
    }

    /// Restore the whole work tree to `tree`; returns the paths that changed.
    ///
    /// # Errors
    /// Git failures.
    pub fn restore_all(&self, tree: &str) -> Result<Vec<String>> {
        let changed = self.files(tree, None)?;
        self.restore(tree, &changed)?;
        Ok(changed)
    }

    fn check_inside(&self, path: &str) -> Result<()> {
        let reject = |why: &str| {
            Err(WorktreeError::Git {
                command: "restore".into(),
                stderr: format!("path escapes the work tree ({why}): {path}"),
            })
        };
        let relative = Path::new(path);
        if path.is_empty() || relative.is_absolute() || relative.has_root() {
            return reject("absolute");
        }
        for component in relative.components() {
            match component {
                // Case-insensitive: `.GIT` is the same directory on macOS and Windows.
                std::path::Component::Normal(name)
                    if name.to_string_lossy().eq_ignore_ascii_case(".git") =>
                {
                    return reject(".git")
                }
                std::path::Component::Normal(_) => {}
                _ => return reject("not a plain relative path"),
            }
        }
        // A symlinked directory on the way must not lead outside. The final component may
        // itself be a symlink (restoring the link, not its target), so only its parent counts.
        let full = self.work_tree.join(relative);
        let parent = full.parent().unwrap_or(&full);
        match crate::workspace::canonicalize_lenient(parent) {
            Ok(real) if real.starts_with(&self.work_tree) => Ok(()),
            _ => reject("outside"),
        }
    }

    fn base_excludes(&self) -> String {
        let mut excludes = String::from("/.git\n/.next-agent/\n");
        if let Ok(project) = std::fs::read_to_string(self.work_tree.join(".git/info/exclude")) {
            excludes.push_str(&project);
            excludes.push('\n');
        }
        excludes
    }

    fn write_excludes(&self, contents: &str) -> Result<()> {
        let info = self.git_dir.join("info");
        std::fs::create_dir_all(&info)
            .and_then(|()| std::fs::write(info.join("exclude"), contents))
            .map_err(|e| WorktreeError::Git {
                command: "write info/exclude".into(),
                stderr: e.to_string(),
            })
    }

    fn pack_when_due(&self) {
        let stamp = self.git_dir.join(SNAPSHOT_PACK_STAMP);
        let now = std::time::SystemTime::now();
        let due = std::fs::metadata(&stamp)
            .and_then(|m| m.modified())
            .map_or(true, |at| {
                now.duration_since(at)
                    .is_ok_and(|age| age.as_secs() >= SNAPSHOT_PACK_INTERVAL_SECS)
            });
        if due {
            let _ = std::fs::write(&stamp, b"");
            let _ = self.git(&["gc", "--auto", "--quiet"]);
        }
    }
}

/// Escape gitignore metacharacters so a path matches literally.
fn escape_exclude(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | '!' | '#' | ' ') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Unified diff parser
// ---------------------------------------------------------------------------------------------

fn parse_range(s: &str) -> (u32, u32) {
    let s = s.trim_start_matches(['-', '+']);
    match s.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let (old_start, old_lines) = parse_range(old);
    let (new_start, new_lines) = parse_range(new);
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        section: section.trim().to_owned(),
        lines: Vec::new(),
    })
}

/// Parse `git diff` output (default prefixes `a/` `b/`) into per-file hunks.
#[must_use]
pub fn parse_unified_diff(raw: &str) -> StructuredDiff {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            // Equal halves ("P b/P") for everything except renames, which override below.
            let path = if rest.len() % 2 == 1 {
                let n = (rest.len() - 3) / 2;
                rest.get(..n).unwrap_or(rest)
            } else {
                rest
            };
            files.push(FileDiff {
                path: path.to_owned(),
                old_path: None,
                status: FileStatus::Modified,
                binary: false,
                additions: 0,
                deletions: 0,
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_hunk && matches!(line.chars().next(), Some(' ' | '+' | '-' | '\\')) {
            let Some(hunk) = file.hunks.last_mut() else {
                continue;
            };
            let (kind, text) = match line.as_bytes()[0] {
                b'+' => (LineKind::Added, &line[1..]),
                b'-' => (LineKind::Removed, &line[1..]),
                b' ' => (LineKind::Context, &line[1..]),
                _ => continue, // "\ No newline at end of file"
            };
            match kind {
                LineKind::Added => file.additions += 1,
                LineKind::Removed => file.deletions += 1,
                LineKind::Context => {}
            }
            hunk.lines.push(DiffLine {
                kind,
                text: text.to_owned(),
            });
            continue;
        }
        if line.starts_with("@@ ") {
            if let Some(h) = parse_hunk_header(line) {
                file.hunks.push(h);
                in_hunk = true;
            }
            continue;
        }
        in_hunk = false;
        if line.starts_with("new file mode") {
            file.status = FileStatus::Added;
        } else if line.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(p.to_owned());
        } else if let Some(p) = line.strip_prefix("rename to ") {
            p.clone_into(&mut file.path);
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            file.binary = true;
        }
    }
    StructuredDiff { files }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let p = std::env::temp_dir().join(format!("na-wt-{}-{n}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p.canonicalize().unwrap())
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn g(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=Tester",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn init_repo() -> TempDir {
        let t = TempDir::new();
        g(t.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(t.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(t.path().join("b.txt"), "bee\n").unwrap();
        g(t.path(), &["add", "-A"]);
        g(t.path(), &["commit", "-q", "-m", "init"]);
        t
    }

    fn tid(n: &str) -> TaskId {
        TaskId::from_seed(n)
    }

    fn snapshot(repo: &Path) -> (String, String, String) {
        (
            g(repo, &["rev-parse", "HEAD"]),
            g(repo, &["status", "--porcelain=v1", "--untracked-files=all"]),
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
        )
    }

    #[test]
    fn non_git_workspace_is_refused_with_typed_error() {
        let t = TempDir::new();
        let err = GitCli
            .ensure_task_worktree(t.path(), &tid("x"))
            .unwrap_err();
        assert!(matches!(err, WorktreeError::NotAGitRepo(_)));
        assert!(err.should_fall_back_to_manual());
    }

    #[test]
    fn repo_without_commits_is_refused() {
        let t = TempDir::new();
        g(t.path(), &["init", "-q", "-b", "main"]);
        let err = GitCli
            .ensure_task_worktree(t.path(), &tid("x"))
            .unwrap_err();
        assert_eq!(err, WorktreeError::NoCommits);
        assert!(err.should_fall_back_to_manual());
    }

    #[test]
    fn subdirectory_of_repo_is_refused() {
        let t = init_repo();
        std::fs::create_dir(t.path().join("sub")).unwrap();
        let err = GitCli
            .ensure_task_worktree(&t.path().join("sub"), &tid("x"))
            .unwrap_err();
        assert!(matches!(err, WorktreeError::NotAGitRepo(_)));
    }

    #[test]
    fn creates_worktree_on_auto_branch_and_is_idempotent() {
        let t = init_repo();
        let id = tid("one");
        let wt = GitCli.ensure_task_worktree(t.path(), &id).unwrap();
        assert_eq!(
            wt.root,
            t.path().join(".next-agent/worktrees").join(id.as_str())
        );
        assert!(wt.branch.starts_with("next-agent/auto-"));
        assert_eq!(wt.base_commit, g(t.path(), &["rev-parse", "HEAD"]).trim());
        assert!(wt.root.join("a.txt").exists());
        assert_eq!(g(&wt.root, &["branch", "--show-current"]).trim(), wt.branch);
        // excluded without touching .gitignore
        assert_eq!(g(t.path(), &["status", "--porcelain"]).trim(), "");
        assert!(!t.path().join(".gitignore").exists());
        let again = GitCli.ensure_task_worktree(t.path(), &id).unwrap();
        assert_eq!(again, wt);
        assert_eq!(GitCli.list(t.path()).unwrap(), vec![wt.clone()]);
        assert_eq!(GitCli.find(t.path(), &id).unwrap(), Some(wt));
    }

    #[test]
    fn identity_validation_rejects_tampered_branch_metadata() {
        let t = init_repo();
        let id = tid("identity");
        let wt = GitCli.ensure_task_worktree(t.path(), &id).unwrap();
        let mut tampered = wt.clone();
        tampered.branch = "next-agent/auto-other".into();
        let err = GitCli.checkpoint(&tampered, "must reject").unwrap_err();
        assert!(matches!(err, WorktreeError::InvalidIdentity { .. }));
        assert!(wt.root.exists());
        GitCli.discard(&wt).unwrap();
    }

    #[test]
    fn identity_validation_rejects_stale_base_metadata() {
        let t = init_repo();
        let id = tid("stale-base");
        let wt = GitCli.ensure_task_worktree(t.path(), &id).unwrap();
        let mut tampered = wt.clone();
        tampered.base_commit = "deadbeef".into();
        let err = GitCli.diff(&tampered).unwrap_err();
        assert!(matches!(err, WorktreeError::InvalidIdentity { .. }));
        GitCli.discard(&wt).unwrap();
    }

    #[test]
    fn auto_task_never_touches_main_tree_until_merge() {
        let t = init_repo();
        let before = snapshot(t.path());
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("iso")).unwrap();
        std::fs::write(wt.root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(wt.root.join("new.txt"), "n\n").unwrap();
        std::fs::remove_file(wt.root.join("b.txt")).unwrap();
        GitCli.checkpoint(&wt, "work").unwrap();
        GitCli.diff(&wt).unwrap();
        GitCli.keep(&wt).unwrap();
        assert_eq!(snapshot(t.path()), before);
        assert!(!t.path().join("new.txt").exists());
        assert!(t.path().join("b.txt").exists());
    }

    #[test]
    fn structured_diff_has_files_and_hunks() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("diff")).unwrap();
        assert!(GitCli.diff(&wt).unwrap().is_empty());
        std::fs::write(wt.root.join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        std::fs::write(wt.root.join("sp ace.txt"), "x\n").unwrap();
        std::fs::remove_file(wt.root.join("b.txt")).unwrap();
        // uncommitted edits are invisible until checkpoint
        assert!(GitCli.diff(&wt).unwrap().is_empty());
        assert!(GitCli.checkpoint(&wt, "edit").unwrap().is_some());
        assert!(GitCli.checkpoint(&wt, "again").unwrap().is_none());
        let d = GitCli.diff(&wt).unwrap();
        let by = |p: &str| {
            d.files
                .iter()
                .find(|f| f.path == p)
                .unwrap_or_else(|| panic!("{p}"))
        };
        let a = by("a.txt");
        assert_eq!(a.status, FileStatus::Modified);
        assert_eq!((a.additions, a.deletions), (2, 1));
        assert_eq!(a.hunks.len(), 1);
        assert_eq!((a.hunks[0].old_start, a.hunks[0].new_start), (1, 1));
        assert!(a.hunks[0].lines.contains(&DiffLine {
            kind: LineKind::Added,
            text: "TWO".into()
        }));
        assert!(a.hunks[0].lines.contains(&DiffLine {
            kind: LineKind::Removed,
            text: "two".into()
        }));
        assert_eq!(by("b.txt").status, FileStatus::Deleted);
        assert_eq!(by("sp ace.txt").status, FileStatus::Added);
    }

    #[test]
    fn diff_detects_renames_and_binary() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("ren")).unwrap();
        std::fs::rename(wt.root.join("a.txt"), wt.root.join("c.txt")).unwrap();
        std::fs::write(wt.root.join("bin.dat"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        GitCli.checkpoint(&wt, "x").unwrap();
        let d = GitCli.diff(&wt).unwrap();
        let r = d.files.iter().find(|f| f.path == "c.txt").unwrap();
        assert_eq!(r.status, FileStatus::Renamed);
        assert_eq!(r.old_path.as_deref(), Some("a.txt"));
        assert!(d.files.iter().find(|f| f.path == "bin.dat").unwrap().binary);
    }

    #[test]
    fn squash_merge_lands_one_commit_and_cleans_up() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("sq")).unwrap();
        std::fs::write(wt.root.join("a.txt"), "x\n").unwrap();
        GitCli.checkpoint(&wt, "c1").unwrap();
        std::fs::write(wt.root.join("c.txt"), "y\n").unwrap(); // uncommitted: merge checkpoints
        let before = g(t.path(), &["rev-list", "--count", "HEAD"]);
        let out = GitCli.merge(&wt, &MergeOptions::default()).unwrap();
        assert_eq!(out.strategy, MergeStrategy::Squash);
        assert_eq!(out.files_changed, vec!["a.txt", "c.txt"]);
        assert_eq!(
            out.commit.as_deref(),
            Some(g(t.path(), &["rev-parse", "HEAD"]).trim())
        );
        let after = g(t.path(), &["rev-list", "--count", "HEAD"]);
        assert_eq!(
            after.trim().parse::<u32>().unwrap(),
            before.trim().parse::<u32>().unwrap() + 1
        );
        assert_eq!(
            std::fs::read_to_string(t.path().join("a.txt")).unwrap(),
            "x\n"
        );
        assert!(t.path().join("c.txt").exists());
        assert!(!wt.root.exists());
        assert_eq!(
            g(t.path(), &["branch", "--list", "next-agent/*"]).trim(),
            ""
        );
        assert_eq!(g(t.path(), &["status", "--porcelain"]).trim(), "");
    }

    #[test]
    fn no_ff_merge_creates_merge_commit() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("nf")).unwrap();
        std::fs::write(wt.root.join("n.txt"), "n\n").unwrap();
        let opts = MergeOptions {
            strategy: MergeStrategy::NoFastForward,
            message: Some("merge it".into()),
        };
        let out = GitCli.merge(&wt, &opts).unwrap();
        assert!(out.commit.is_some());
        assert_eq!(
            g(t.path(), &["rev-list", "--parents", "-n1", "HEAD"])
                .split_whitespace()
                .count(),
            3
        );
        assert_eq!(
            g(t.path(), &["log", "-1", "--format=%s"]).trim(),
            "merge it"
        );
        assert!(t.path().join("n.txt").exists());
    }

    #[test]
    fn merge_of_unchanged_task_is_a_noop_and_cleans_up() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("noop")).unwrap();
        let out = GitCli.merge(&wt, &MergeOptions::default()).unwrap();
        assert_eq!(out.commit, None);
        assert!(!wt.root.exists());
    }

    #[test]
    fn merge_conflict_is_typed_and_main_tree_restored() {
        for strategy in [MergeStrategy::Squash, MergeStrategy::NoFastForward] {
            let t = init_repo();
            let wt = GitCli
                .ensure_task_worktree(t.path(), &tid(&format!("cf{strategy:?}")))
                .unwrap();
            std::fs::write(wt.root.join("a.txt"), "agent\ntwo\nthree\n").unwrap();
            // base moves in main with a conflicting edit
            std::fs::write(t.path().join("a.txt"), "user\ntwo\nthree\n").unwrap();
            g(t.path(), &["commit", "-qam", "user edit"]);
            let head = g(t.path(), &["rev-parse", "HEAD"]);
            let err = GitCli
                .merge(
                    &wt,
                    &MergeOptions {
                        strategy,
                        message: None,
                    },
                )
                .unwrap_err();
            assert_eq!(
                err,
                WorktreeError::MergeConflict {
                    files: vec!["a.txt".into()]
                }
            );
            assert_eq!(g(t.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(g(t.path(), &["status", "--porcelain"]).trim(), "");
            assert_eq!(
                std::fs::read_to_string(t.path().join("a.txt")).unwrap(),
                "user\ntwo\nthree\n"
            );
            // task is intact and can still be discarded
            assert!(wt.root.exists());
            GitCli.discard(&wt).unwrap();
        }
    }

    #[test]
    fn base_moved_without_conflict_merges_three_way() {
        let t = init_repo();
        let wt = GitCli
            .ensure_task_worktree(t.path(), &tid("moved"))
            .unwrap();
        std::fs::write(wt.root.join("a.txt"), "one\ntwo\nthree\nagent\n").unwrap();
        std::fs::write(t.path().join("b.txt"), "user\n").unwrap();
        g(t.path(), &["commit", "-qam", "user"]);
        GitCli.merge(&wt, &MergeOptions::default()).unwrap();
        assert_eq!(
            std::fs::read_to_string(t.path().join("a.txt")).unwrap(),
            "one\ntwo\nthree\nagent\n"
        );
        assert_eq!(
            std::fs::read_to_string(t.path().join("b.txt")).unwrap(),
            "user\n"
        );
    }

    #[test]
    fn dirty_main_tree_overlapping_files_blocks_merge_unrelated_does_not() {
        let t = init_repo();
        let wt = GitCli
            .ensure_task_worktree(t.path(), &tid("dirty"))
            .unwrap();
        std::fs::write(wt.root.join("a.txt"), "agent\n").unwrap();
        // overlapping uncommitted edit
        std::fs::write(t.path().join("a.txt"), "mine\n").unwrap();
        let err = GitCli.merge(&wt, &MergeOptions::default()).unwrap_err();
        assert_eq!(
            err,
            WorktreeError::DirtyMainTree {
                files: vec!["a.txt".into()]
            }
        );
        assert_eq!(
            std::fs::read_to_string(t.path().join("a.txt")).unwrap(),
            "mine\n"
        );
        assert!(wt.root.exists());
        // unrelated dirty file is preserved and does not block
        g(t.path(), &["checkout", "a.txt"]);
        std::fs::write(t.path().join("b.txt"), "mine too\n").unwrap();
        GitCli.merge(&wt, &MergeOptions::default()).unwrap();
        assert_eq!(
            std::fs::read_to_string(t.path().join("b.txt")).unwrap(),
            "mine too\n"
        );
        assert_eq!(
            std::fs::read_to_string(t.path().join("a.txt")).unwrap(),
            "agent\n"
        );
        // and the user's edit is not swept into the merge commit
        assert_eq!(
            g(t.path(), &["show", "--name-only", "--format=", "HEAD"]).trim(),
            "a.txt"
        );
    }

    #[test]
    fn staged_changes_in_main_block_merge() {
        let t = init_repo();
        let wt = GitCli
            .ensure_task_worktree(t.path(), &tid("staged"))
            .unwrap();
        std::fs::write(wt.root.join("a.txt"), "agent\n").unwrap();
        std::fs::write(t.path().join("b.txt"), "staged\n").unwrap();
        g(t.path(), &["add", "b.txt"]);
        let err = GitCli.merge(&wt, &MergeOptions::default()).unwrap_err();
        assert_eq!(
            err,
            WorktreeError::DirtyMainTree {
                files: vec!["b.txt".into()]
            }
        );
    }

    #[test]
    fn detached_head_blocks_merge() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("det")).unwrap();
        std::fs::write(wt.root.join("a.txt"), "agent\n").unwrap();
        g(t.path(), &["checkout", "-q", "--detach"]);
        assert_eq!(
            GitCli.merge(&wt, &MergeOptions::default()).unwrap_err(),
            WorktreeError::DetachedHead
        );
    }

    #[test]
    fn discard_removes_worktree_and_branch_and_is_idempotent() {
        let t = init_repo();
        let before = snapshot(t.path());
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("disc")).unwrap();
        std::fs::write(wt.root.join("a.txt"), "gone\n").unwrap(); // uncommitted too
        GitCli.discard(&wt).unwrap();
        GitCli.discard(&wt).unwrap();
        assert!(!wt.root.exists());
        assert_eq!(
            g(t.path(), &["branch", "--list", "next-agent/*"]).trim(),
            ""
        );
        assert_eq!(GitCli.find(t.path(), &wt.task_id).unwrap(), None);
        assert_eq!(snapshot(t.path()), before);
    }

    #[test]
    fn keep_preserves_branch_and_lists_it() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("keep")).unwrap();
        std::fs::write(wt.root.join("a.txt"), "kept\n").unwrap();
        GitCli.keep(&wt).unwrap();
        assert!(wt.root.exists());
        let listed = GitCli.list(t.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].branch, wt.branch);
        assert_eq!(GitCli.diff(&listed[0]).unwrap().files.len(), 1);
    }

    #[test]
    fn prune_drops_records_of_deleted_worktrees() {
        let t = init_repo();
        let wt = GitCli.ensure_task_worktree(t.path(), &tid("orph")).unwrap();
        std::fs::remove_dir_all(&wt.root).unwrap();
        GitCli.prune(t.path()).unwrap();
        assert!(!g(t.path(), &["worktree", "list"]).contains("worktrees"));
    }

    #[test]
    fn parser_handles_multiple_hunks_and_no_newline_marker() {
        let raw = "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@ fn a\n-a\n+b\n c\n@@ -10 +10 @@\n-x\n\\ No newline at end of file\n+y\n";
        let d = parse_unified_diff(raw);
        assert_eq!(d.files.len(), 1);
        let f = &d.files[0];
        assert_eq!(f.hunks.len(), 2);
        assert_eq!(f.hunks[0].section, "fn a");
        assert_eq!((f.hunks[1].old_start, f.hunks[1].old_lines), (10, 1));
        assert_eq!((f.additions, f.deletions), (2, 2));
    }

    // --- shadow snapshots ------------------------------------------------------------------

    #[test]
    fn shadow_capture_and_restore_round_trip() {
        let repo = init_repo();
        let data = TempDir::new();
        let before = snapshot(repo.path());
        let shadow = ShadowRepo::open(data.path(), repo.path(), "t1").unwrap();
        assert!(shadow.git_dir.starts_with(data.path()));
        let tree = shadow.capture().unwrap();
        assert_eq!(tree.len(), 40);
        // Capturing again with no change yields the same tree.
        assert_eq!(shadow.capture().unwrap(), tree);

        std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
        std::fs::create_dir_all(repo.path().join("new/dir")).unwrap();
        std::fs::write(repo.path().join("new/dir/c.txt"), "c\n").unwrap();
        std::fs::remove_file(repo.path().join("b.txt")).unwrap();
        let mut changed = shadow.files(&tree, None).unwrap();
        changed.sort();
        assert_eq!(changed, ["a.txt", "b.txt", "new/dir/c.txt"]);
        let diff = shadow.diff(&tree, None).unwrap();
        assert_eq!(diff.files.len(), 3);

        let mut restored = shadow.restore_all(&tree).unwrap();
        restored.sort();
        assert_eq!(restored, ["a.txt", "b.txt", "new/dir/c.txt"]);
        assert!(!repo.path().join("new").exists());
        // The user's repo (HEAD, index, status, content) is exactly as before.
        assert_eq!(snapshot(repo.path()), before);
    }

    #[test]
    fn shadow_snapshots_respect_ignores_and_size_limit() {
        let repo = init_repo();
        let data = TempDir::new();
        std::fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(repo.path().join("target")).unwrap();
        std::fs::write(repo.path().join("target/out.bin"), "x").unwrap();
        std::fs::create_dir_all(repo.path().join(".next-agent/agents")).unwrap();
        std::fs::write(repo.path().join(".next-agent/agents/a.md"), "x").unwrap();
        let big = vec![b'x'; usize::try_from(SNAPSHOT_MAX_UNTRACKED_BYTES).unwrap() + 1];
        std::fs::write(repo.path().join("big.dat"), big).unwrap();
        let shadow = ShadowRepo::open(data.path(), repo.path(), "t1").unwrap();
        let tree = shadow.capture().unwrap();
        let listed = shadow
            .git(&["ls-tree", "-r", "--name-only", &tree])
            .unwrap();
        let listed: Vec<&str> = listed.lines().collect();
        assert!(listed.contains(&"a.txt"));
        assert!(listed.contains(&".gitignore"));
        assert!(!listed.iter().any(|p| p.starts_with("target")));
        assert!(!listed.iter().any(|p| p.starts_with(".next-agent")));
        assert!(!listed.contains(&"big.dat"));
    }

    #[test]
    fn shadow_restore_rejects_paths_outside_the_work_tree() {
        let repo = init_repo();
        let data = TempDir::new();
        let shadow = ShadowRepo::open(data.path(), repo.path(), "t1").unwrap();
        let tree = shadow.capture().unwrap();
        for bad in [
            "../escape.txt",
            "/etc/passwd",
            "a/../../x",
            ".git/config",
            ".GIT/hooks/pre-commit",
            "sub/.Git/x",
            "",
        ] {
            let err = shadow.restore(&tree, &[bad.to_string()]).unwrap_err();
            assert!(err.to_string().contains("escapes"), "{bad}: {err}");
        }
        #[cfg(unix)]
        {
            let outside = TempDir::new();
            std::os::unix::fs::symlink(outside.path(), repo.path().join("link")).unwrap();
            let err = shadow
                .restore(&tree, &["link/file.txt".to_string()])
                .unwrap_err();
            assert!(err.to_string().contains("escapes"), "{err}");
        }
        // One bad path means nothing is restored.
        std::fs::write(repo.path().join("a.txt"), "edited\n").unwrap();
        assert!(shadow
            .restore(&tree, &["a.txt".to_string(), "../x".to_string()])
            .is_err());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
            "edited\n"
        );
    }

    #[test]
    fn shadow_repo_location_is_keyed_by_work_tree() {
        let data = Path::new("/data");
        let a = ShadowRepo::location(data, Path::new("/w/one"), "t1");
        let b = ShadowRepo::location(data, Path::new("/w/two"), "t1");
        let other_task = ShadowRepo::location(data, Path::new("/w/one"), "t2");
        assert_ne!(a, b);
        assert_ne!(a, other_task);
        assert!(a.starts_with("/data/snapshot"));
        assert_eq!(a, ShadowRepo::location(data, Path::new("/w/one"), "t1"));
    }
}
