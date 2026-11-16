//! Task orchestration for the desktop app: the M1 exit scenario end to end.
//!
//! `task_start` → (auto: git worktree) → `spawn_task` with real tools and the
//! router-backed or scripted agent → events streamed to the UI → on finish:
//! checkpoint + diff for review, task record in the vault → Merge / Discard /
//! Keep from the reviewer (merge re-documents the record as merged).

use crate::review::{diff_dto, ReviewOutcome};
use crate::service::{mode_name, parse_mode, AppService, Sink, TaskRequest};
use crate::wire::{envelope, StreamMessage};
use next_agent_core::builtin::RoleResolver;
use next_agent_core::profiles::{load_profiles, AgentProfile};
use next_agent_core::router::ReqwestFetcher;
use next_agent_core::vault::{FileStat, TaskDoc, TaskMeta, VaultWriter};
use next_agent_core::{
    context_for, spawn_task_with, AgentEvent, AgentExecutor, AgentStep, Approval, BuiltinExecutor,
    EventKind, GitCli, GitSnapshots, LocalTools, MergeOptions, MockExecutor, RunContext, TaskId,
    TaskLaunch, TaskOptions, TaskSpec, TaskWorktree, ToolExecutor, ToolRequest, ToolResult,
    WorktreeProvider,
};
use next_agent_permission_engine::Mode;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Characters of a prompt kept as a task title.
pub(crate) const TITLE_CHARS: usize = 60;

/// Smallest known context window across the chat role's targets (0 = unknown,
/// so compaction runs only on a provider overflow).
pub(crate) fn context_window(gateway: &next_agent_core::router::Gateway) -> u64 {
    let config = gateway.config();
    let Ok(plan) = config.resolve("", "chat") else {
        return 0;
    };
    plan.targets
        .iter()
        .filter_map(|t| {
            let provider = config
                .accounts(&t.provider)
                .first()
                .map_or_else(|| t.provider.clone(), |c| c.provider.clone());
            config.pricing.context_length(&provider, &t.model)
        })
        .min()
        .unwrap_or(0)
}

/// An agent profile the UI can offer.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProfileDto {
    pub name: String,
    pub description: String,
    pub builtin: bool,
}

impl From<&AgentProfile> for ProfileDto {
    fn from(p: &AgentProfile) -> Self {
        Self {
            name: p.name.clone(),
            description: p.description.clone(),
            builtin: p.builtin,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProfileList {
    pub profiles: Vec<ProfileDto>,
    pub warnings: Vec<String>,
}

/// Result of "Undo last step".
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct UndoDto {
    pub seq: i64,
    pub call_id: String,
    pub tool: String,
    pub restored: Vec<String>,
    pub remaining: usize,
}

/// Everything kept about a finished task for review and re-documenting.
#[derive(Debug, Clone)]
pub struct FinishedTask {
    pub meta: TaskMeta,
    pub events: Vec<AgentEvent>,
    pub calls: Vec<(String, ToolRequest)>,
    pub diff_stats: Vec<FileStat>,
    pub worktree: Option<TaskWorktree>,
}

type Calls = Arc<Mutex<Vec<(String, ToolRequest)>>>;

/// Builds a scripted task's steps once its sandbox root (the task worktree)
/// is known.
pub type Script = Box<dyn FnOnce(&Path) -> Result<Vec<AgentStep>, String> + Send>;

/// Remembers the requests the broker let through, so the vault record can
/// name files and commands (events carry only tool names and JSON args).
struct Recording {
    inner: Box<dyn ToolExecutor + Send>,
    calls: Calls,
}

impl Recording {
    fn record(&self, call_id: &str, request: &ToolRequest) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((call_id.to_string(), request.clone()));
        }
    }
}

impl ToolExecutor for Recording {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult {
        self.record(call_id, request);
        self.inner.execute(call_id, request)
    }

    fn execute_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> ToolResult {
        self.record(call_id, request);
        self.inner.execute_in(ctx, call_id, request)
    }
}

/// How a task was started, beyond [`TaskRequest`]: cron runs and the
/// background review use these (`Default` = an ordinary task from the UI).
#[derive(Debug, Clone, Default)]
pub struct TaskExtras {
    /// `task` (default), `cron` or `review`.
    pub source: Option<&'static str>,
    /// Skills whose SKILL.md goes in front of the prompt (cron jobs).
    pub skills: Vec<String>,
    /// Gateway role for the model (`chat` when unset).
    pub model_role: Option<String>,
    /// Workspace-relative folder the agent starts in.
    pub workdir: Option<String>,
}

pub(crate) struct Plan {
    pub(crate) spec: TaskSpec,
    pub(crate) mode: Mode,
    pub(crate) workspace_id: String,
    workspace_name: String,
    /// Sandbox root: the worktree in auto mode, else the workspace.
    pub(crate) root: PathBuf,
    /// The workspace itself (agent profiles live in its `.next-agent/agents`).
    pub(crate) workspace_root: PathBuf,
    /// Agent profile picked in the UI (`None` = default for the mode).
    profile: Option<String>,
    worktree: Option<TaskWorktree>,
    notices: Vec<String>,
    // Memory and learning (crate::learning).
    pub(crate) extras: TaskExtras,
    /// The real agent ran (not the scripted demo).
    pub(crate) builtin: bool,
    /// Where relative paths resolve: the sandbox root, or `workdir` in it.
    start_dir: PathBuf,
}

/// Extras for a task started from a chat channel (`crate::channels`): the
/// channel tool profile (it only tightens) and the approval timeout (no
/// answer from the desktop or overlay in time = deny).
pub struct ChannelLaunch {
    pub profile: AgentProfile,
    pub approval_timeout: Duration,
}

impl AppService {
    /// Start a task on background threads, streaming to `sink`.
    ///
    /// # Errors
    /// Unknown workspace or mode, git failures other than "not a repo", or
    /// database errors.
    pub fn task_start(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
    ) -> Result<String, String> {
        self.task_start_inner(request, sink, TaskExtras::default(), None, None)
    }

    /// [`Self::task_start`] with a fixed script instead of the agent
    /// (Design Studio writes). A scripted task must run in a task worktree:
    /// its steps still go through the broker and the permission engine, and
    /// the result is reviewed and merged like any other task.
    ///
    /// # Errors
    /// As [`Self::task_start`], plus: no worktree for a scripted task, or the
    /// script could not be built.
    pub fn task_start_scripted(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
        script: Script,
    ) -> Result<String, String> {
        self.task_start_inner(request, sink, TaskExtras::default(), None, Some(script))
    }

    /// [`AppService::task_start`] for cron runs and other non-UI starts.
    ///
    /// # Errors
    /// See [`AppService::task_start`]; also a bad `workdir`.
    pub fn task_start_with(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
        extras: TaskExtras,
    ) -> Result<String, String> {
        self.task_start_inner(request, sink, extras, None, None)
    }

    /// [`AppService::task_start`] as a channel task. A channel task runs
    /// only in manual or plan mode, under the channel profile.
    ///
    /// # Errors
    /// See [`AppService::task_start`]; also a channel task in any other mode.
    pub fn task_start_channel(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
        channel: &ChannelLaunch,
    ) -> Result<String, String> {
        self.task_start_inner(request, sink, TaskExtras::default(), Some(channel), None)
    }

    #[allow(clippy::too_many_lines)] // One start path for UI, cron, channel and scripted tasks.
    fn task_start_inner(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
        extras: TaskExtras,
        channel: Option<&ChannelLaunch>,
        script: Option<Script>,
    ) -> Result<String, String> {
        if channel.is_some() {
            next_agent_channels::config::guard_mode(parse_mode(&request.mode)?)?;
        }
        let mut plan = self.plan(request, extras)?;
        let task_id = plan.spec.id.as_str().to_string();
        let scripted = match script {
            None => None,
            Some(build) => {
                let Some(wt) = &plan.worktree else {
                    return Err("Design Studio changes are written in a task worktree, which needs a git repository with at least one commit. Commit the project first.".into());
                };
                match build(&plan.root) {
                    Ok(steps) => Some(steps),
                    Err(error) => {
                        let _ = GitCli.discard(wt);
                        return Err(error);
                    }
                }
            }
        };

        if let Some(wt) = &plan.worktree {
            sink(StreamMessage::Worktree {
                task_id: task_id.clone(),
                branch: wt.branch.clone(),
                root: wt.root.display().to_string(),
            });
        }
        let learning = self.learning_for(&plan);
        let (executor, agent_notice) = if let Some(steps) = scripted {
            (
                Box::new(MockExecutor::new(steps)) as Box<dyn AgentExecutor + Send>,
                None,
            )
        } else {
            let (executor, notice, builtin) =
                self.choose_executor(&plan, &task_id, &learning, channel.map(|c| &c.profile));
            plan.builtin = builtin;
            (executor, notice)
        };
        let executor: Box<dyn AgentExecutor + Send> = match channel {
            Some(c) => Box::new(next_agent_channels::ChannelExecutor::new(
                executor,
                c.profile.clone(),
            )),
            None => executor,
        };
        for text in plan.notices.iter().chain(agent_notice.iter()) {
            sink(StreamMessage::Notice {
                task_id: task_id.clone(),
                tone: "info",
                text: text.clone(),
            });
        }

        let calls: Calls = Arc::default();
        let tools = Recording {
            inner: Box::new(next_agent_core::learning::LearningTools::new(
                Box::new(local_tools()),
                learning,
            )),
            calls: Arc::clone(&calls),
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        // Per-step undo: capture the sandbox before each mutating step.
        let data_dir = self
            .db_path()
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        let snapshots = GitSnapshots::open(&data_dir, &plan.root, &task_id)
            .ok()
            .map(|s| Box::new(s) as Box<dyn next_agent_core::StepSnapshots + Send>);
        let handle = spawn_task_with(
            TaskLaunch {
                db_path: self.db_path().to_path_buf(),
                spec: plan.spec.clone(),
                mode: plan.mode,
                context: context_for(plan.root.clone(), plan.workspace_id.clone(), home),
                executor,
                tools: Box::new(tools),
            },
            TaskOptions {
                snapshots,
                approval_timeout: channel.map(|c| c.approval_timeout),
                untrusted_origin: channel.is_some(),
                ..TaskOptions::default()
            },
        );
        self.running()?.insert(task_id.clone(), handle.control());

        let service = Arc::clone(self);
        let id = task_id.clone();
        let join = std::thread::Builder::new()
            .name(format!("ui-{task_id}"))
            .spawn(move || {
                let mut events = Vec::new();
                while let Ok(event) = handle.events().recv() {
                    sink(StreamMessage::Event {
                        event: envelope(&event),
                    });
                    events.push(event);
                }
                if let Err(error) = handle.join() {
                    sink(StreamMessage::Notice {
                        task_id: id.clone(),
                        tone: "error",
                        text: format!("task could not run: {error}"),
                    });
                }
                if let Ok(mut running) = service.running() {
                    running.remove(&id);
                }
                let calls = calls.lock().map_or_else(|_| Vec::new(), |c| c.clone());
                service.finish(&plan, events.clone(), calls, &sink);
                service.after_task(&plan, &events, &sink);
            })
            .map_err(|e| format!("could not start task thread: {e}"))?;
        self.joins()?.insert(task_id.clone(), join);
        Ok(task_id)
    }

    /// Block until a started task has been fully finalized (tests, headless).
    pub fn task_wait(&self, task_id: &str) {
        let join = self.joins().ok().and_then(|mut j| j.remove(task_id));
        if let Some(join) = join {
            let _ = join.join();
        }
    }

    /// Cancel a running task; its pending approvals resolve as denied.
    pub fn task_cancel(&self, task_id: &str) {
        if let Some(control) = self.running().ok().and_then(|r| r.get(task_id).cloned()) {
            control.cancel();
        }
    }

    /// Answer an approval card.
    ///
    /// # Errors
    /// Unknown task/approval or invalid choice.
    pub fn approval_respond(
        &self,
        task_id: &str,
        approval_id: &str,
        choice: &str,
    ) -> Result<(), String> {
        let approval = parse_choice(choice)?;
        let control = self
            .running()?
            .get(task_id)
            .cloned()
            .ok_or_else(|| format!("task {task_id} is not running"))?;
        control
            .respond(approval_id, approval)
            .map_err(|e| format!("{e:?}"))
    }

    /// Merge, discard or keep a finished auto-mode task's worktree.
    ///
    /// # Errors
    /// No worktree for the task, unknown action, or git errors (dirty main
    /// tree, merge conflict — the main tree is left untouched).
    pub fn review_action(&self, task_id: &str, action: &str) -> Result<ReviewOutcome, String> {
        let worktree = self
            .finished()?
            .get(task_id)
            .and_then(|f| f.worktree.clone())
            .ok_or_else(|| format!("no worktree to review for task {task_id}"))?;
        let mut outcome = match action {
            "merge" => {
                let options = MergeOptions {
                    message: Some(format!("next-agent: {}", self.title_of(task_id))),
                    ..MergeOptions::default()
                };
                let merged = GitCli
                    .merge(&worktree, &options)
                    .map_err(|e| e.to_string())?;
                ReviewOutcome::merged(&merged)
            }
            "discard" => {
                GitCli.discard(&worktree).map_err(|e| e.to_string())?;
                ReviewOutcome::simple("discard")
            }
            "keep" => {
                GitCli.keep(&worktree).map_err(|e| e.to_string())?;
                return Ok(ReviewOutcome::simple("keep"));
            }
            other => return Err(format!("unknown review action `{other}`")),
        };
        // Merge and discard end the worktree; merge also updates the record.
        let doc = {
            let mut finished = self.finished()?;
            let entry = finished
                .get_mut(task_id)
                .ok_or_else(|| format!("unknown task {task_id}"))?;
            entry.worktree = None;
            entry.meta.merged = action == "merge";
            entry.clone()
        };
        if action == "merge" {
            // The merge already happened; a stale record must not undo that,
            // but the user should know the vault copy says "not merged".
            if let Err(error) = self.document(&doc) {
                tracing::warn!(%task_id, kind = "vault_write", %error, "task record not updated after merge");
                outcome.notice = Some(format!(
                    "Merged, but the task record could not be updated: {error}"
                ));
            }
        }
        Ok(outcome)
    }

    fn title_of(&self, task_id: &str) -> String {
        self.finished()
            .ok()
            .and_then(|f| {
                f.get(task_id)
                    .map(|t| t.meta.prompt.lines().next().unwrap_or("").to_string())
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("task {task_id}"))
    }

    fn plan(&self, request: &TaskRequest, extras: TaskExtras) -> Result<Plan, String> {
        let mut mode = parse_mode(&request.mode)?;
        let workspace = self.workspace(&request.workspace_id)?;
        let n = self.counter().fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = TaskId::from_seed(&format!("{}:{nanos}:{n}", workspace.id));
        let workspace_root = PathBuf::from(&workspace.path);
        // Check `workdir` before anything is created for the task.
        start_dir(&workspace_root, extras.workdir.as_deref())?;

        let mut notices = Vec::new();
        let mut worktree = None;
        if mode == Mode::Auto {
            match GitCli.ensure_task_worktree(&workspace_root, &id) {
                Ok(wt) => worktree = Some(wt),
                Err(e) if e.should_fall_back_to_manual() => {
                    mode = Mode::Manual;
                    notices.push(format!(
                        "Auto mode needs a git repository with at least one commit, so this task runs in Manual mode ({e})."
                    ));
                }
                Err(e) => return Err(format!("could not create the task worktree: {e}")),
            }
        }
        let root = worktree
            .as_ref()
            .map_or_else(|| workspace_root.clone(), |wt| wt.root.clone());

        let name = mode_name(mode);
        let title: String = request.prompt.chars().take(TITLE_CHARS).collect();
        self.store()?
            .create_task(id.as_str(), &workspace.id, &title, &request.prompt, name)
            .map_err(|e| e.to_string())?;
        let model = match &extras.model_role {
            Some(role) => self
                .gateway(role)
                .ok()
                .and_then(|g| g.config().role_target(role))
                .unwrap_or_else(|| self.chat_target()),
            None => self.chat_target(),
        };
        let prompt = self.with_skills(&workspace_root, &extras.skills, &request.prompt);
        // An untracked folder may be missing from the worktree: start at its root.
        let start_dir = start_dir(&root, extras.workdir.as_deref()).unwrap_or_else(|why| {
            notices.push(format!("{why}; starting at the workspace root."));
            root.clone()
        });
        Ok(Plan {
            spec: TaskSpec {
                id,
                prompt,
                mode: name,
                model,
            },
            mode,
            workspace_id: workspace.id,
            workspace_name: workspace.name,
            root,
            workspace_root,
            profile: request.profile.clone().filter(|p| !p.trim().is_empty()),
            worktree,
            notices,
            extras,
            builtin: false,
            start_dir,
        })
    }

    /// The real agent on the built-in gateway when the `chat` role (or the
    /// default model) has a usable provider, otherwise the scripted demo
    /// agent (no LLM needed). No external router is required.
    /// Smart combos keep one model per task (`task_id` is the session).
    fn choose_executor(
        self: &Arc<Self>,
        plan: &Plan,
        task_id: &str,
        learning: &next_agent_core::learning::Learning,
        ancestor: Option<&AgentProfile>,
    ) -> (Box<dyn AgentExecutor + Send>, Option<String>, bool) {
        let root = &plan.start_dir;
        let gateway = match &plan.extras.model_role {
            Some(role) => self.gateway(role),
            None => self.chat_gateway(),
        };
        let gateway = match gateway {
            Ok(gateway) => gateway,
            Err(why) => {
                return (
                    Box::new(MockExecutor::new(demo_script(root))),
                    Some(format!("Running the scripted demo agent because {why}.")),
                    false,
                )
            }
        };
        let mut notices = Vec::new();
        let (profiles, errors) = load_profiles(&plan.workspace_root);
        for error in errors {
            notices.push(format!(
                "Agent profile `{}` ignored: {}",
                error.name, error.message
            ));
        }
        let window = context_window(&gateway);
        let mut executor = BuiltinExecutor::new(Arc::new(gateway.with_session(task_id)), root)
            .with_profiles(profiles.clone())
            .with_context_window(window)
            .with_role_resolver(self.role_resolver(task_id))
            .with_learning(learning.clone());
        if let Some(recorder) = self.session_recorder(plan) {
            executor = executor.with_transcript(Box::new(recorder));
        }
        if let Some(profile) = ancestor {
            executor = executor.with_ancestor_profile(profile.clone());
        }
        // Compaction summaries use the `summary` role (falls back to chat).
        if let Ok(summary) = self.gateway("summary") {
            executor = executor.with_summary_client(Arc::new(summary.with_session(task_id)), None);
        }
        if let Some(name) = &plan.profile {
            match profiles
                .iter()
                .find(|p| &p.name == name && p.mode.primary())
            {
                Some(profile) => executor = executor.with_profile(profile.clone()),
                None => notices.push(format!(
                    "Agent profile `{name}` not found; using the default."
                )),
            }
        }
        let notice = (!notices.is_empty()).then(|| notices.join(" "));
        (Box::new(executor), notice, true)
    }

    /// Maps a profile's `model_role` to that role's gateway and target.
    fn role_resolver(self: &Arc<Self>, task_id: &str) -> RoleResolver {
        let service = Arc::clone(self);
        let session = task_id.to_string();
        Arc::new(move |role: &str| {
            let gateway = service.gateway(role).ok()?;
            let model = gateway.config().role_target(role)?;
            let client: Arc<dyn next_agent_core::llm::CompletionClient> =
                Arc::new(gateway.with_session(session.clone()));
            Some((client, model))
        })
    }

    /// Agent profiles a task can run as (primary or all), with load warnings.
    ///
    /// # Errors
    /// Unknown workspace.
    pub fn agent_profiles(&self, workspace_id: &str) -> Result<ProfileList, String> {
        let workspace = self.workspace(workspace_id)?;
        let (profiles, errors) = load_profiles(Path::new(&workspace.path));
        Ok(ProfileList {
            profiles: profiles
                .iter()
                .filter(|p| p.mode.primary())
                .map(ProfileDto::from)
                .collect(),
            warnings: errors
                .iter()
                .map(|e| format!("{}: {}", e.name, e.message))
                .collect(),
        })
    }

    /// Undo the last mutating step of a finished task.
    ///
    /// # Errors
    /// The task is still running, has nothing to undo, or git failed.
    pub fn task_undo_step(&self, task_id: &str) -> Result<UndoDto, String> {
        if self.running()?.contains_key(task_id) {
            return Err("stop or wait for the task before undoing its steps".into());
        }
        let store = self.open_store()?;
        let outcome =
            next_agent_core::undo_last_step(&store, task_id).map_err(|e| e.to_string())?;
        Ok(UndoDto {
            seq: outcome.seq,
            call_id: outcome.call_id,
            tool: outcome.tool,
            restored: outcome.restored,
            remaining: outcome.remaining,
        })
    }

    /// How many steps of a task can still be undone.
    ///
    /// # Errors
    /// Database errors.
    pub fn task_undoable(&self, task_id: &str) -> Result<usize, String> {
        let store = self.open_store()?;
        next_agent_core::undoable_steps(&store, task_id)
            .map(|steps| steps.len())
            .map_err(|e| e.to_string())
    }

    /// Checkpoint + diff the worktree, write the vault record, remember the task.
    fn finish(
        &self,
        plan: &Plan,
        events: Vec<AgentEvent>,
        calls: Vec<(String, ToolRequest)>,
        sink: &Sink,
    ) {
        let task_id = plan.spec.id.as_str().to_string();
        let notice = |tone: &'static str, text: String| {
            sink(StreamMessage::Notice {
                task_id: task_id.clone(),
                tone,
                text,
            });
        };

        let (worktree, diff_stats) = match &plan.worktree {
            Some(wt) => Self::review_worktree(&task_id, wt, sink),
            None => (None, Vec::new()),
        };

        let started = events.first().map_or_else(String::new, |e| e.ts.clone());
        let finished_at = events.last().map_or_else(String::new, |e| e.ts.clone());
        let finished = FinishedTask {
            meta: TaskMeta {
                id: plan.spec.id.clone(),
                title: None,
                prompt: plan.spec.prompt.clone(),
                workspace: plan.workspace_name.clone(),
                workspace_root: Some(plan.root.clone()),
                mode: plan.spec.mode.to_string(),
                branch: plan
                    .worktree
                    .as_ref()
                    .map_or_else(String::new, |w| w.branch.clone()),
                base: plan
                    .worktree
                    .as_ref()
                    .map_or_else(String::new, |w| w.base_commit.chars().take(12).collect()),
                merged: false,
                model: if plan.spec.model.is_empty() {
                    "mock".into()
                } else {
                    plan.spec.model.clone()
                },
                started,
                finished: finished_at,
            },
            events,
            calls,
            diff_stats,
            worktree,
        };
        if let Ok(mut all) = self.finished() {
            all.insert(task_id.clone(), finished.clone());
        }
        match self.document(&finished) {
            Ok(Some(path)) => sink(StreamMessage::Documented {
                task_id: task_id.clone(),
                path,
            }),
            Ok(None) => notice(
                "info",
                "Set a vault folder in Settings to keep task records.".into(),
            ),
            Err(e) => notice("error", format!("Could not write the task record: {e}")),
        }
    }

    /// Checkpoint and diff a task's worktree for review. Returns the worktree
    /// to keep (`None` when it had no changes and was removed) and the diff
    /// stats for the record.
    fn review_worktree(
        task_id: &str,
        wt: &TaskWorktree,
        sink: &Sink,
    ) -> (Option<TaskWorktree>, Vec<FileStat>) {
        let notice = |tone: &'static str, text: String| {
            sink(StreamMessage::Notice {
                task_id: task_id.to_string(),
                tone,
                text,
            });
        };
        let checkpoint = GitCli.checkpoint(wt, &format!("next-agent: checkpoint {task_id}"));
        match checkpoint.and_then(|_| GitCli.diff(wt)) {
            Ok(diff) if diff.is_empty() => {
                // Nothing to review: drop the worktree so it doesn't linger.
                let _ = GitCli.discard(wt);
                notice(
                    "info",
                    "No file changes; the task worktree was removed.".into(),
                );
                (None, Vec::new())
            }
            Ok(diff) => {
                let diff_stats = diff
                    .files
                    .iter()
                    .map(|f| FileStat {
                        path: f.path.clone(),
                        added: f.additions,
                        removed: f.deletions,
                    })
                    .collect();
                sink(StreamMessage::Review {
                    task_id: task_id.to_string(),
                    branch: wt.branch.clone(),
                    files: diff_dto(&diff),
                });
                (Some(wt.clone()), diff_stats)
            }
            Err(e) => {
                notice("error", format!("Could not read the task's changes: {e}"));
                (Some(wt.clone()), Vec::new())
            }
        }
    }

    /// Write (or rewrite) the task record. `Ok(None)` when no vault is set.
    fn document(&self, task: &FinishedTask) -> Result<Option<String>, String> {
        let Some(vault) = self.vault_get()? else {
            return Ok(None);
        };
        let doc = TaskDoc {
            meta: task.meta.clone(),
            events: task.events.clone(),
            calls: task.calls.clone(),
            diff_stats: task.diff_stats.clone(),
        };
        let store = self.open_store()?;
        let outcome = VaultWriter::new(vault)
            .document_task(&store, &doc, None)
            .map_err(|e| e.to_string())?;
        Ok(Some(outcome.record_path.display().to_string()))
    }
}

/// `workdir` inside the sandbox root (must exist; no escaping it).
fn start_dir(root: &Path, workdir: Option<&str>) -> Result<PathBuf, String> {
    let Some(dir) = workdir.filter(|d| !d.trim().is_empty()) else {
        return Ok(root.to_path_buf());
    };
    let root_real = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", root.display()))?;
    let real = root
        .join(dir)
        .canonicalize()
        .map_err(|_| format!("workdir `{dir}` does not exist in the workspace"))?;
    if !real.starts_with(&root_real) || !real.is_dir() {
        return Err(format!(
            "workdir `{dir}` must be a folder inside the workspace"
        ));
    }
    Ok(real)
}

pub(crate) fn local_tools() -> LocalTools {
    match ReqwestFetcher::new(Duration::from_secs(20), 2 * 1024 * 1024) {
        Ok(fetcher) => LocalTools::new().with_http(Arc::new(fetcher)),
        Err(_) => LocalTools::new(),
    }
}

fn parse_choice(choice: &str) -> Result<Approval, String> {
    match choice {
        "allow" => Ok(Approval::Allow),
        "deny" => Ok(Approval::Deny),
        "always" | "always_for_workspace" => Ok(Approval::AlwaysForWorkspace),
        other => Err(format!("unknown approval choice `{other}`")),
    }
}

/// Scripted agent for running without an LLM: read, write a demo note, list.
#[must_use]
pub fn demo_script(root: &Path) -> Vec<AgentStep> {
    vec![
        AgentStep::Emit(EventKind::Message {
            text: "Demo agent: reading the project, then adding a note.".into(),
        }),
        AgentStep::Call {
            call_id: "call_1".into(),
            request: ToolRequest::ListDir {
                path: root.to_path_buf(),
            },
        },
        AgentStep::Call {
            call_id: "call_2".into(),
            request: ToolRequest::Write {
                path: root.join("NEXT_AGENT_DEMO.md"),
                contents: "# Next Agent demo\n\nWritten by the scripted demo agent.\n".into(),
            },
        },
        AgentStep::Call {
            call_id: "call_3".into(),
            request: ToolRequest::Exec {
                command: "ls".into(),
                cwd: root.to_path_buf(),
            },
        },
    ]
}
