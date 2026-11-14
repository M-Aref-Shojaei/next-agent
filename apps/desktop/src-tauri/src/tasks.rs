//! Task orchestration for the desktop app: the M1 exit scenario end to end.
//!
//! `task_start` → (auto: git worktree) → `spawn_task` with real tools and the
//! router-backed or scripted agent → events streamed to the UI → on finish:
//! checkpoint + diff for review, task record in the vault → Merge / Discard /
//! Keep from the reviewer (merge re-documents the record as merged).

use crate::review::{diff_dto, ReviewOutcome};
use crate::service::{mode_name, parse_mode, AppService, Sink, TaskRequest};
use crate::wire::{envelope, StreamMessage};
use next_agent_core::router::ReqwestFetcher;
use next_agent_core::vault::{FileStat, TaskDoc, TaskMeta, VaultWriter};
use next_agent_core::{
    context_for, spawn_task, AgentEvent, AgentExecutor, AgentStep, Approval, BuiltinExecutor,
    EventKind, GitCli, LocalTools, MergeOptions, MockExecutor, RunContext, Store, TaskId,
    TaskLaunch, TaskSpec, TaskWorktree, ToolExecutor, ToolRequest, ToolResult, WorktreeProvider,
};
use next_agent_permission_engine::Mode;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

struct Plan {
    spec: TaskSpec,
    mode: Mode,
    workspace_id: String,
    workspace_name: String,
    /// Sandbox root: the worktree in auto mode, else the workspace.
    root: PathBuf,
    worktree: Option<TaskWorktree>,
    notices: Vec<String>,
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
        let plan = self.plan(request)?;
        let task_id = plan.spec.id.as_str().to_string();

        if let Some(wt) = &plan.worktree {
            sink(StreamMessage::Worktree {
                task_id: task_id.clone(),
                branch: wt.branch.clone(),
                root: wt.root.display().to_string(),
            });
        }
        let (executor, agent_notice) = self.choose_executor(&plan.root, &task_id);
        for text in plan.notices.iter().chain(agent_notice.iter()) {
            sink(StreamMessage::Notice {
                task_id: task_id.clone(),
                tone: "info",
                text: text.clone(),
            });
        }

        let calls: Calls = Arc::default();
        let tools = Recording {
            inner: Box::new(local_tools()),
            calls: Arc::clone(&calls),
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let handle = spawn_task(TaskLaunch {
            db_path: self.db_path().to_path_buf(),
            spec: plan.spec.clone(),
            mode: plan.mode,
            context: context_for(plan.root.clone(), plan.workspace_id.clone(), home),
            executor,
            tools: Box::new(tools),
        });
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
                service.finish(&plan, events, calls, &sink);
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
        let outcome = match action {
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
            let _ = self.document(&doc);
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

    fn plan(&self, request: &TaskRequest) -> Result<Plan, String> {
        let mut mode = parse_mode(&request.mode)?;
        let workspace = self.workspace(&request.workspace_id)?;
        let n = self.counter().fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = TaskId::from_seed(&format!("{}:{nanos}:{n}", workspace.id));
        let workspace_root = PathBuf::from(&workspace.path);

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
            .map_or(workspace_root, |wt| wt.root.clone());

        let name = mode_name(mode);
        let title: String = request.prompt.chars().take(60).collect();
        self.store()?
            .create_task(id.as_str(), &workspace.id, &title, &request.prompt, name)
            .map_err(|e| e.to_string())?;
        let model = self.chat_target();
        Ok(Plan {
            spec: TaskSpec {
                id,
                prompt: request.prompt.clone(),
                mode: name,
                model,
            },
            mode,
            workspace_id: workspace.id,
            workspace_name: workspace.name,
            root,
            worktree,
            notices,
        })
    }

    /// The real agent on the built-in gateway when the `chat` role (or the
    /// default model) has a usable provider, otherwise the scripted demo
    /// agent (no LLM needed). No external router is required.
    /// Smart combos keep one model per task (`task_id` is the session).
    fn choose_executor(
        &self,
        root: &Path,
        task_id: &str,
    ) -> (Box<dyn AgentExecutor + Send>, Option<String>) {
        match self.chat_gateway() {
            Ok(gateway) => (
                Box::new(BuiltinExecutor::new(
                    Arc::new(gateway.with_session(task_id)),
                    root,
                )),
                None,
            ),
            Err(why) => (
                Box::new(MockExecutor::new(demo_script(root))),
                Some(format!("Running the scripted demo agent because {why}.")),
            ),
        }
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

        let mut diff_stats = Vec::new();
        let mut worktree = plan.worktree.clone();
        if let Some(wt) = &plan.worktree {
            let checkpoint = GitCli.checkpoint(wt, &format!("next-agent: checkpoint {task_id}"));
            match checkpoint.and_then(|_| GitCli.diff(wt)) {
                Ok(diff) if diff.is_empty() => {
                    // Nothing to review: drop the worktree so it doesn't linger.
                    let _ = GitCli.discard(wt);
                    worktree = None;
                    notice(
                        "info",
                        "No file changes; the task worktree was removed.".into(),
                    );
                }
                Ok(diff) => {
                    diff_stats = diff
                        .files
                        .iter()
                        .map(|f| FileStat {
                            path: f.path.clone(),
                            added: f.additions,
                            removed: f.deletions,
                        })
                        .collect();
                    sink(StreamMessage::Review {
                        task_id: task_id.clone(),
                        branch: wt.branch.clone(),
                        files: diff_dto(&diff),
                    });
                }
                Err(e) => notice("error", format!("Could not read the task's changes: {e}")),
            }
        }

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
        let store = Store::open(self.db_path()).map_err(|e| e.to_string())?;
        let outcome = VaultWriter::new(vault)
            .document_task(&store, &doc, None)
            .map_err(|e| e.to_string())?;
        Ok(Some(outcome.record_path.display().to_string()))
    }
}

fn local_tools() -> LocalTools {
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
