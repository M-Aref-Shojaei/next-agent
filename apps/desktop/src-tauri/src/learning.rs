//! Memory, skills, the background review and cron jobs in the desktop app
//! (the logic is in `next_agent_core::learning`, ported from Hermes Agent).
//!
//! - Memory lives per workspace in `<app data>/memory/<workspace id>/`.
//! - Skills: `<workspace>/.next-agent/skills/` and `<vault>/skills/`.
//! - After a task completes, [`AppService::after_task`] may start the
//!   background review on its own thread; what it saved shows up in the
//!   task's timeline as a notice.
//! - [`start_scheduler`] ticks the cron table every 20 s while the app runs
//!   and starts a task per due run. Channels can reuse
//!   [`AppService::schedule_job`], [`AppService::list_jobs`] and
//!   [`AppService::cancel_job`].
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use crate::service::{AppService, Sink, TaskRequest};
use crate::tasks::{context_window, local_tools, Plan, TaskExtras};
use crate::wire::{StreamMessage, EVENT_TASK_STARTED, EVENT_TASK_STREAM};
use next_agent_core::learning::cron::{self, DueRun, JobSpec};
use next_agent_core::learning::memory::{self, MemoryStore, Target};
use next_agent_core::learning::review::{self, BudgetClient, ReviewApprover};
use next_agent_core::learning::sessions::{self, SessionRecorder};
use next_agent_core::learning::skills::{self, Scope, SkillRoot, Skills, Transition, UsageFacts};
use next_agent_core::learning::{LearnChange, Learning, LearningTools, ToolSet};
use next_agent_core::store::{CronJobRow, SessionRow};
use next_agent_core::{
    context_for, spawn_task_with, AgentEvent, BuiltinExecutor, TaskId, TaskLaunch, TaskOptions,
    TaskSpec,
};
use next_agent_permission_engine::Mode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

const REVIEW_KEY: &str = "learning.background_review";
const CURATOR_KEY: &str = "learning.curator_last_run";
/// Seconds between scheduler ticks.
const TICK: Duration = Duration::from_secs(20);

/// Settings → Memory and skills.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningSettings {
    /// Run the background review after each completed task (default on).
    pub background_review: bool,
}

/// One memory file for the editor.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFileDto {
    pub target: &'static str,
    /// Entries joined by the `§` separator line, as on disk.
    pub text: String,
    pub entries: usize,
    pub used: usize,
    pub limit: usize,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDto {
    pub memory: MemoryFileDto,
    pub user: MemoryFileDto,
}

/// A skill in the Settings list (active or archived).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub scope: Scope,
    /// The skills folder (pin/restore address skills by folder + name).
    pub root: String,
    pub state: String,
    pub pinned: bool,
    pub created_by: Option<String>,
    pub use_count: i64,
    pub view_count: i64,
    pub patch_count: i64,
    pub last_activity_at: Option<i64>,
}

/// What a curator run did.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CuratorReport {
    pub checked: usize,
    pub marked_stale: Vec<String>,
    pub archived: Vec<String>,
    pub reactivated: Vec<String>,
}

/// A cron job for the UI and for other modules.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CronJobDto {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    pub schedule_label: String,
    pub skills: Vec<String>,
    pub model_role: Option<String>,
    pub mode: String,
    pub workdir: Option<String>,
    pub timezone: Option<String>,
    pub grace_minutes: Option<i64>,
    pub repeat: Option<i64>,
    pub run_count: i64,
    pub state: String,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_task_id: Option<String>,
    pub origin: String,
}

impl From<&CronJobRow> for CronJobDto {
    fn from(row: &CronJobRow) -> Self {
        let label = cron::zone(row.timezone.as_deref())
            .and_then(|tz| {
                cron::parse_schedule(&row.schedule, row.created_at, &tz)
                    .map(|s| cron::describe(&s, &tz))
            })
            .unwrap_or_else(|_| row.schedule.clone());
        Self {
            id: row.id.clone(),
            workspace_id: row.workspace_id.clone(),
            name: row.name.clone(),
            prompt: row.prompt.clone(),
            schedule: row.schedule.clone(),
            schedule_label: label,
            skills: cron::job_skills(row),
            model_role: row.model_role.clone(),
            mode: row.mode.clone(),
            workdir: row.workdir.clone(),
            timezone: row.timezone.clone(),
            grace_minutes: row.grace_minutes,
            repeat: row.repeat_times,
            run_count: row.run_count,
            state: row.state.clone(),
            next_run_at: row.next_run_at,
            last_run_at: row.last_run_at,
            last_status: row.last_status.clone(),
            last_error: row.last_error.clone(),
            last_task_id: row.last_task_id.clone(),
            origin: row.origin.clone(),
        }
    }
}

/// Create or edit a job from the Settings page (`id` = edit).
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct CronInput {
    pub id: Option<String>,
    pub workspace_id: String,
    pub name: Option<String>,
    pub prompt: String,
    pub schedule: String,
    pub skills: Vec<String>,
    pub model_role: Option<String>,
    pub mode: Option<String>,
    pub workdir: Option<String>,
    pub timezone: Option<String>,
    pub grace_minutes: Option<i64>,
    pub repeat: Option<i64>,
}

impl CronInput {
    fn spec(&self) -> JobSpec {
        let blank = |v: &Option<String>| v.clone().filter(|s| !s.trim().is_empty());
        JobSpec {
            workspace_id: self.workspace_id.clone(),
            name: blank(&self.name),
            prompt: self.prompt.clone(),
            schedule: self.schedule.clone(),
            skills: self
                .skills
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            model_role: blank(&self.model_role),
            mode: blank(&self.mode),
            workdir: blank(&self.workdir),
            timezone: blank(&self.timezone),
            grace_minutes: self.grace_minutes,
            repeat: self.repeat,
            origin: "user".into(),
        }
    }
}

fn memory_file(store: &MemoryStore, target: Target) -> MemoryFileDto {
    let entries = store.entries(target).unwrap_or_default();
    MemoryFileDto {
        target: target.as_str(),
        text: entries.join(memory::ENTRY_DELIMITER),
        entries: entries.len(),
        used: memory::used_chars(&entries),
        limit: target.limit(),
        path: store.path(target).display().to_string(),
    }
}

impl AppService {
    fn data_dir(&self) -> PathBuf {
        self.db_path()
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }

    /// `<app data>/memory/<workspace id>/`.
    pub(crate) fn memory_dir(&self, workspace_id: &str) -> PathBuf {
        // Workspace ids are hex hashes; anything else is flattened.
        let safe: String = workspace_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        self.data_dir().join("memory").join(safe)
    }

    /// The workspace's skills folder, then the vault's.
    pub(crate) fn skills_for(&self, workspace_root: &Path) -> Skills {
        let mut roots = vec![SkillRoot {
            scope: Scope::Workspace,
            path: workspace_root.join(".next-agent").join("skills"),
        }];
        if let Ok(Some(vault)) = self.vault_get() {
            roots.push(SkillRoot {
                scope: Scope::User,
                path: PathBuf::from(vault).join("skills"),
            });
        }
        Skills::new(roots)
    }

    /// Learning tools for a task. Cron runs skip memory (Hermes: cron
    /// sessions do not write the user's memory).
    pub(crate) fn learning_for(&self, plan: &Plan) -> Learning {
        let cron = plan.extras.source == Some("cron");
        self.learning(
            &plan.workspace_id,
            &plan.workspace_root,
            &plan.root,
            plan.spec.id.as_str(),
            ToolSet {
                memory: !cron,
                ..ToolSet::ALL
            },
        )
    }

    /// Learning tools for one task or review fork of a workspace.
    fn learning(
        &self,
        workspace_id: &str,
        workspace_root: &Path,
        sandbox_root: &Path,
        session_id: &str,
        tools: ToolSet,
    ) -> Learning {
        Learning {
            workspace_id: workspace_id.to_string(),
            sandbox_root: sandbox_root.to_path_buf(),
            memory_dir: self.memory_dir(workspace_id),
            skills: self.skills_for(workspace_root),
            db_path: self.db_path().to_path_buf(),
            session_id: Some(session_id.to_string()),
            tools,
            review: false,
        }
    }

    /// The session-store recorder for a task (`None` if the database fails).
    pub(crate) fn session_recorder(&self, plan: &Plan) -> Option<SessionRecorder> {
        let id = plan.spec.id.as_str().to_string();
        let row = SessionRow {
            id: id.clone(),
            workspace_id: plan.workspace_id.clone(),
            task_id: Some(id),
            source: plan.extras.source.unwrap_or("task").to_string(),
            title: Some(title_of(&plan.spec.prompt, 80)),
            started_at: sessions::now(),
            ..SessionRow::default()
        };
        SessionRecorder::open(self.db_path(), &row)
            .inspect_err(|error| tracing::warn!(%error, "session store unavailable"))
            .ok()
    }

    /// Put the named skills' SKILL.md in front of a cron prompt.
    pub(crate) fn with_skills(
        &self,
        workspace_root: &Path,
        names: &[String],
        prompt: &str,
    ) -> String {
        if names.is_empty() {
            return prompt.to_string();
        }
        let skills = self.skills_for(workspace_root);
        let mut blocks = Vec::new();
        for name in names {
            match skills.view_json(name, None) {
                Ok(view) => blocks.push(format!(
                    "<skill name=\"{name}\">\n{}\n</skill>",
                    view["content"].as_str().unwrap_or_default()
                )),
                Err(error) => blocks.push(format!("(skill `{name}` could not be loaded: {error})")),
            }
        }
        format!(
            "Use these skills for this task:\n\n{}\n\n{prompt}",
            blocks.join("\n\n")
        )
    }

    // -- after a task --------------------------------------------------------

    /// Close the task's session and maybe start the background review.
    pub(crate) fn after_task(self: &Arc<Self>, plan: &Plan, events: &[AgentEvent], sink: &Sink) {
        let status = events.last().map_or("failed", |e| e.kind.kind());
        if let Ok(store) = self.store() {
            let _ = store.session_end(plan.spec.id.as_str(), sessions::now(), status);
        }
        let completed = status == "task_completed";
        let tool_calls = events
            .iter()
            .filter(|e| e.kind.kind() == "tool_requested")
            .count();
        let source = plan.extras.source.unwrap_or("task");
        if !plan.builtin
            || !self.review_enabled()
            || !review::worth_reviewing(source, completed, tool_calls)
        {
            return;
        }
        let service = Arc::clone(self);
        let job = ReviewJob {
            task_id: plan.spec.id.as_str().to_string(),
            title: title_of(&plan.spec.prompt, 60),
            workspace_id: plan.workspace_id.clone(),
            workspace_root: plan.workspace_root.clone(),
        };
        let sink = Arc::clone(sink);
        let _ = std::thread::Builder::new()
            .name(format!("review-{}", job.task_id))
            .spawn(move || {
                let notice = match service.run_review(&job) {
                    Ok(changes) if changes.is_empty() => None,
                    Ok(changes) => {
                        let mut lines: Vec<String> =
                            changes.into_iter().map(|c| c.summary).collect();
                        lines.dedup();
                        Some((
                            "ok",
                            format!("Background review saved: {}", lines.join(" · ")),
                        ))
                    }
                    Err(error) => Some(("info", format!("Background review did not run: {error}"))),
                };
                if let Some((tone, text)) = notice {
                    sink(StreamMessage::Notice {
                        task_id: job.task_id.clone(),
                        tone,
                        text,
                    });
                }
                service.maybe_curate();
            });
    }

    fn review_enabled(&self) -> bool {
        self.learning_settings_get()
            .is_ok_and(|s| s.background_review)
    }

    /// Run the review fork for a finished task and wait for it. Returns
    /// what it wrote (each write also sits in the audit log).
    fn run_review(&self, job: &ReviewJob) -> Result<Vec<LearnChange>, String> {
        let gateway = self.gateway("summary").or_else(|_| self.chat_gateway())?;
        let model = gateway
            .config()
            .role_target("summary")
            .unwrap_or_else(|| self.chat_target());
        let budget = review::token_budget(context_window(&gateway));
        let messages = self.open_store().and_then(|s| {
            s.session_messages(&job.task_id, true)
                .map_err(|e| e.to_string())
        })?;
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let max_chars = usize::try_from(budget / 2)
            .unwrap_or(usize::MAX)
            .saturating_mul(4)
            .min(200_000);
        let prompt =
            review::task_prompt(&review::render_transcript(&messages, max_chars), &job.title);
        let id = TaskId::from_seed(&format!("review:{}:{}", job.task_id, sessions::now()));
        self.store()?
            .create_task(
                id.as_str(),
                &job.workspace_id,
                &format!("Background review: {}", job.title),
                &prompt,
                "manual",
            )
            .map_err(|e| e.to_string())?;
        let session = SessionRow {
            id: id.as_str().to_string(),
            workspace_id: job.workspace_id.clone(),
            task_id: Some(id.as_str().to_string()),
            parent_session_id: Some(job.task_id.clone()),
            source: "review".into(),
            title: Some(format!("Background review: {}", job.title)),
            started_at: sessions::now(),
            ..SessionRow::default()
        };
        let learning = Learning {
            review: true,
            ..self.learning(
                &job.workspace_id,
                &job.workspace_root,
                &job.workspace_root,
                id.as_str(),
                ToolSet {
                    session_search: false,
                    ..ToolSet::ALL
                },
            )
        };
        let client = BudgetClient::new(Arc::new(gateway.with_session(id.as_str())), budget);
        let mut executor = BuiltinExecutor::new(Arc::new(client), &job.workspace_root)
            .with_profile(review::profile())
            .with_step_budget(review::MAX_ITERATIONS)
            .with_context_window(0)
            .with_learning(learning.clone());
        if let Ok(recorder) = SessionRecorder::open(self.db_path(), &session) {
            executor = executor.with_transcript(Box::new(recorder));
        }
        let tools = LearningTools::new(Box::new(local_tools()), learning);
        let changes = tools.changes();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let handle = spawn_task_with(
            TaskLaunch {
                db_path: self.db_path().to_path_buf(),
                spec: TaskSpec {
                    id,
                    prompt,
                    mode: "manual",
                    model,
                },
                mode: Mode::Manual,
                context: context_for(job.workspace_root.clone(), job.workspace_id.clone(), home),
                executor: Box::new(executor),
                tools: Box::new(tools),
            },
            TaskOptions {
                approver: Some(Box::new(ReviewApprover)),
                ..TaskOptions::default()
            },
        );
        while handle.events().recv().is_ok() {}
        handle.join()?;
        let changes = changes.lock().map_or_default(|c| c.clone());
        Ok(changes)
    }

    // -- settings ------------------------------------------------------------

    pub fn learning_settings_get(&self) -> Result<LearningSettings, String> {
        let raw = self
            .store()?
            .get_setting(REVIEW_KEY)
            .map_err(|e| e.to_string())?;
        Ok(LearningSettings {
            background_review: raw.as_deref() != Some("false"),
        })
    }

    pub fn learning_settings_set(&self, settings: &LearningSettings) -> Result<(), String> {
        self.store()?
            .set_setting(
                REVIEW_KEY,
                if settings.background_review {
                    "true"
                } else {
                    "false"
                },
            )
            .map_err(|e| e.to_string())
    }

    // -- memory --------------------------------------------------------------

    pub fn memory_get(&self, workspace_id: &str) -> Result<MemoryDto, String> {
        self.workspace(workspace_id)?;
        let store = MemoryStore::new(self.memory_dir(workspace_id));
        Ok(MemoryDto {
            memory: memory_file(&store, Target::Memory),
            user: memory_file(&store, Target::User),
        })
    }

    /// Replace one file from the editor (the user's own edit; the cap holds).
    pub fn memory_save(
        &self,
        workspace_id: &str,
        target: &str,
        text: &str,
    ) -> Result<MemoryDto, String> {
        self.workspace(workspace_id)?;
        let target = Target::parse(target).ok_or("target must be memory or user")?;
        MemoryStore::new(self.memory_dir(workspace_id)).save_text(target, text)?;
        self.memory_get(workspace_id)
    }

    // -- skills --------------------------------------------------------------

    fn workspace_skills(&self, workspace_id: &str) -> Result<Skills, String> {
        let workspace = self.workspace(workspace_id)?;
        Ok(self.skills_for(Path::new(&workspace.path)))
    }

    /// Every skill of the workspace and the vault, archived ones included.
    pub fn skills_overview(&self, workspace_id: &str) -> Result<Vec<SkillDto>, String> {
        let skills = self.workspace_skills(workspace_id)?;
        let found = skills.discover();
        let store = self.store()?;
        let mut out = Vec::new();
        for root in skills.roots() {
            let key = root.path.display().to_string();
            let usage = store.skill_usage(&key).map_err(|e| e.to_string())?;
            let row = |name: &str| usage.iter().find(|u| u.name == name).cloned();
            let dto = |name: String,
                       description: String,
                       category: Option<String>,
                       state: Option<&str>| {
                let u = row(&name).unwrap_or_default();
                SkillDto {
                    state: state.map_or_else(
                        || {
                            if u.state.is_empty() {
                                "active".into()
                            } else {
                                u.state.clone()
                            }
                        },
                        str::to_string,
                    ),
                    name,
                    description,
                    category,
                    scope: root.scope,
                    root: key.clone(),
                    pinned: u.pinned,
                    created_by: u.created_by.clone(),
                    use_count: u.use_count,
                    view_count: u.view_count,
                    patch_count: u.patch_count,
                    last_activity_at: u.last_activity_at(),
                }
            };
            for info in found.iter().filter(|s| s.root == root.path).cloned() {
                out.push(dto(info.name, info.description, info.category, None));
            }
            for name in skills::archived(&root.path) {
                out.push(dto(name, String::new(), None, Some("archived")));
            }
        }
        Ok(out)
    }

    /// A skill's SKILL.md text.
    pub fn skill_read(&self, workspace_id: &str, name: &str) -> Result<String, String> {
        let view = self.workspace_skills(workspace_id)?.view_json(name, None)?;
        Ok(view["content"].as_str().unwrap_or_default().to_string())
    }

    fn known_root(&self, workspace_id: &str, root: &str) -> Result<PathBuf, String> {
        self.workspace_skills(workspace_id)?
            .roots()
            .iter()
            .find(|r| r.path.display().to_string() == root)
            .map(|r| r.path.clone())
            .ok_or_else(|| format!("{root} is not a skills folder of this workspace"))
    }

    pub fn skill_pin(
        &self,
        workspace_id: &str,
        root: &str,
        name: &str,
        pinned: bool,
    ) -> Result<(), String> {
        self.known_root(workspace_id, root)?;
        skills::validate_name(name)?;
        self.store()?
            .skill_set_pinned(root, name, pinned, sessions::now())
            .map_err(|e| e.to_string())
    }

    /// Move a skill to `.archive/` (never deletes).
    pub fn skill_archive(&self, workspace_id: &str, name: &str) -> Result<(), String> {
        let skill = self
            .workspace_skills(workspace_id)?
            .find(name)
            .ok_or_else(|| format!("no skill named {name}"))?;
        skills::archive_dir(&skill.root, &skill.dir, &skill.name)?;
        self.store()?
            .skill_set_state(
                &skill.root.display().to_string(),
                &skill.name,
                "archived",
                sessions::now(),
            )
            .map_err(|e| e.to_string())
    }

    pub fn skill_restore(&self, workspace_id: &str, root: &str, name: &str) -> Result<(), String> {
        let path = self.known_root(workspace_id, root)?;
        skills::restore(&path, name)?;
        self.store()?
            .skill_set_state(root, name, "active", sessions::now())
            .map_err(|e| e.to_string())
    }

    /// Run the curator over every workspace's skills and the vault's.
    /// Only agent-created, unpinned skills are touched; archiving is the
    /// most it does.
    pub fn curator_run(&self) -> Result<CuratorReport, String> {
        let now = sessions::now();
        let mut report = CuratorReport::default();
        let mut roots: Vec<SkillRoot> = Vec::new();
        for workspace in self.store()?.list_workspaces().map_err(|e| e.to_string())? {
            for root in self.skills_for(Path::new(&workspace.path)).roots() {
                if !roots.contains(root) {
                    roots.push(root.clone());
                }
            }
        }
        skills::clear_cache();
        for root in roots {
            let key = root.path.display().to_string();
            let library = Skills::new(vec![root.clone()]);
            let found = library.discover();
            let usage = self.store()?.skill_usage(&key).map_err(|e| e.to_string())?;
            let facts: Vec<UsageFacts> = found
                .iter()
                .filter_map(|s| usage.iter().find(|u| u.name == s.name))
                .map(|u| UsageFacts {
                    name: u.name.clone(),
                    agent_created: u.created_by.as_deref() == Some("agent"),
                    pinned: u.pinned,
                    state: u.state.clone(),
                    use_count: u.use_count,
                    last_activity_at: u.last_activity_at(),
                    created_at: u.created_at,
                })
                .collect();
            report.checked += facts.len();
            for (name, transition) in skills::curate(&facts, now) {
                let store = self.store()?;
                match transition {
                    Transition::MarkStale => {
                        let _ = store.skill_set_state(&key, &name, "stale", now);
                        report.marked_stale.push(name);
                    }
                    Transition::Reactivate => {
                        let _ = store.skill_set_state(&key, &name, "active", now);
                        report.reactivated.push(name);
                    }
                    Transition::Archive => {
                        if let Some(skill) = found.iter().find(|s| s.name == name) {
                            if skills::archive_dir(&skill.root, &skill.dir, &skill.name).is_ok() {
                                let _ = store.skill_set_state(&key, &name, "archived", now);
                                report.archived.push(name);
                            }
                        }
                    }
                }
            }
        }
        let _ = self.store()?.set_setting(CURATOR_KEY, &now.to_string());
        Ok(report)
    }

    /// Run the curator when the last run is older than its interval.
    pub(crate) fn maybe_curate(&self) {
        let now = sessions::now();
        let last = self
            .store()
            .ok()
            .and_then(|s| s.get_setting(CURATOR_KEY).ok().flatten())
            .and_then(|v| v.parse::<i64>().ok());
        match last {
            // First sight: start the clock instead of curating at once.
            None => {
                if let Ok(store) = self.store() {
                    let _ = store.set_setting(CURATOR_KEY, &now.to_string());
                }
            }
            Some(last) if now - last >= skills::CURATOR_INTERVAL_HOURS * 3_600 => {
                if let Err(error) = self.curator_run() {
                    tracing::warn!(%error, "skills curator failed");
                }
            }
            Some(_) => {}
        }
    }

    // -- cron: the scheduling API -------------------------------------------

    /// Schedule a job (API for channels and the Settings page).
    pub fn schedule_job(&self, spec: &JobSpec) -> Result<CronJobDto, String> {
        self.workspace(&spec.workspace_id)?;
        let row = cron::schedule_job(&*self.store()?, spec, sessions::now())?;
        Ok(CronJobDto::from(&row))
    }

    /// Jobs, optionally of one workspace.
    pub fn list_jobs(&self, workspace_id: Option<&str>) -> Result<Vec<CronJobDto>, String> {
        Ok(cron::list_jobs(&*self.store()?, workspace_id)?
            .iter()
            .map(CronJobDto::from)
            .collect())
    }

    /// Delete a job. Returns whether it existed.
    pub fn cancel_job(&self, id: &str) -> Result<bool, String> {
        cron::cancel_job(&*self.store()?, id)
    }

    pub fn cron_save(&self, input: &CronInput) -> Result<CronJobDto, String> {
        let spec = input.spec();
        match input.id.as_deref().filter(|id| !id.is_empty()) {
            Some(id) => {
                self.workspace(&spec.workspace_id)?;
                let row = cron::update_job(&*self.store()?, id, &spec, sessions::now())?;
                Ok(CronJobDto::from(&row))
            }
            None => self.schedule_job(&spec),
        }
    }

    pub fn cron_pause(&self, id: &str, paused: bool) -> Result<CronJobDto, String> {
        let row = cron::set_paused(&*self.store()?, id, paused, sessions::now())?;
        Ok(CronJobDto::from(&row))
    }

    /// Start one claimed run as its own task. Returns the task id.
    pub fn cron_dispatch(self: &Arc<Self>, run: &DueRun, sink: Sink) -> Result<String, String> {
        let job = &run.job;
        let now = sessions::now();
        // The mode is checked again here, not only when the job was saved:
        // a row edited behind the app's back must not run with more power.
        let outcome = if matches!(job.mode.as_str(), "plan" | "manual" | "auto") {
            let request = TaskRequest {
                workspace_id: job.workspace_id.clone(),
                prompt: job.prompt.clone(),
                mode: job.mode.clone(),
                profile: None,
            };
            let extras = TaskExtras {
                source: Some("cron"),
                skills: cron::job_skills(job),
                model_role: job.model_role.clone(),
                workdir: job.workdir.clone(),
            };
            self.task_start_with(&request, sink, extras)
        } else {
            Err(format!("cron jobs never run in `{}` mode", job.mode))
        };
        let (status, error, task) = match &outcome {
            Ok(task_id) => ("started", None, Some(task_id.as_str())),
            Err(error) => ("error", Some(error.as_str()), None),
        };
        let _ = self
            .store()?
            .cron_record_run(&job.id, now, status, error, task);
        outcome
    }

    /// Claim due runs (at most once each) and start them.
    pub fn cron_tick(
        self: &Arc<Self>,
        sink_for: &dyn Fn(&CronJobRow) -> Sink,
    ) -> Vec<Result<String, String>> {
        let due = match self.store().and_then(|s| cron::tick(&s, sessions::now())) {
            Ok(due) => due,
            Err(error) => return vec![Err(error)],
        };
        due.iter()
            .map(|run| self.cron_dispatch(run, sink_for(&run.job)))
            .collect()
    }
}

/// The first line of a prompt, cut to `n` characters.
fn title_of(prompt: &str, n: usize) -> String {
    prompt
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(n)
        .collect()
}

struct ReviewJob {
    task_id: String,
    title: String,
    workspace_id: String,
    workspace_root: PathBuf,
}

/// Stream a cron task to every window and announce it like a UI task.
fn cron_sink(app: &AppHandle, job: &CronJobRow) -> Sink {
    let handle = app.clone();
    let announce = app.clone();
    let title = format!("⏰ {}", job.name);
    let mode = job.mode.clone();
    let announced = Arc::new(std::sync::atomic::AtomicBool::new(false));
    Arc::new(move |message: StreamMessage| {
        if !announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
            if let Some(task_id) = task_of(&message) {
                let _ = announce.emit(
                    EVENT_TASK_STARTED,
                    serde_json::json!({"taskId": task_id, "title": title, "mode": mode, "source": "cron"}),
                );
            }
        }
        let _ = handle.emit(EVENT_TASK_STREAM, &message);
    })
}

fn task_of(message: &StreamMessage) -> Option<String> {
    match message {
        StreamMessage::Event { event } => event["task_id"].as_str().map(str::to_string),
        StreamMessage::Worktree { task_id, .. }
        | StreamMessage::Notice { task_id, .. }
        | StreamMessage::Review { task_id, .. }
        | StreamMessage::Documented { task_id, .. } => Some(task_id.clone()),
    }
}

/// Tick the cron table while the app runs (jobs never fire when it is
/// closed; a missed slot inside its catch-up window runs once on start).
pub fn start_scheduler(app: AppHandle, service: Arc<AppService>) {
    let _ = std::thread::Builder::new()
        .name("cron".into())
        .spawn(move || {
            service.maybe_curate();
            loop {
                for result in service.cron_tick(&|job| cron_sink(&app, job)) {
                    if let Err(error) = result {
                        tracing::warn!(%error, "cron run did not start");
                    }
                }
                std::thread::sleep(TICK);
            }
        });
}

type Svc<'a> = State<'a, Arc<AppService>>;

#[tauri::command]
pub fn learning_settings_get(service: Svc) -> Result<LearningSettings, String> {
    service.learning_settings_get()
}

#[tauri::command]
pub fn learning_settings_set(service: Svc, settings: LearningSettings) -> Result<(), String> {
    service.learning_settings_set(&settings)
}

#[tauri::command]
pub fn memory_get(service: Svc, workspace_id: String) -> Result<MemoryDto, String> {
    service.memory_get(&workspace_id)
}

#[tauri::command]
pub fn memory_save(
    service: Svc,
    workspace_id: String,
    target: String,
    text: String,
) -> Result<MemoryDto, String> {
    service.memory_save(&workspace_id, &target, &text)
}

#[tauri::command]
pub fn skills_overview(service: Svc, workspace_id: String) -> Result<Vec<SkillDto>, String> {
    service.skills_overview(&workspace_id)
}

#[tauri::command]
pub fn skill_read(service: Svc, workspace_id: String, name: String) -> Result<String, String> {
    service.skill_read(&workspace_id, &name)
}

#[tauri::command]
pub fn skill_pin(
    service: Svc,
    workspace_id: String,
    root: String,
    name: String,
    pinned: bool,
) -> Result<(), String> {
    service.skill_pin(&workspace_id, &root, &name, pinned)
}

#[tauri::command]
pub fn skill_archive(service: Svc, workspace_id: String, name: String) -> Result<(), String> {
    service.skill_archive(&workspace_id, &name)
}

#[tauri::command]
pub fn skill_restore(
    service: Svc,
    workspace_id: String,
    root: String,
    name: String,
) -> Result<(), String> {
    service.skill_restore(&workspace_id, &root, &name)
}

#[tauri::command]
pub fn curator_run(service: Svc) -> Result<CuratorReport, String> {
    service.curator_run()
}

#[tauri::command]
pub fn cron_jobs(service: Svc, workspace_id: Option<String>) -> Result<Vec<CronJobDto>, String> {
    service.list_jobs(workspace_id.as_deref())
}

#[tauri::command]
pub fn cron_save(service: Svc, input: CronInput) -> Result<CronJobDto, String> {
    service.cron_save(&input)
}

#[tauri::command]
pub fn cron_pause(service: Svc, id: String, paused: bool) -> Result<CronJobDto, String> {
    service.cron_pause(&id, paused)
}

#[tauri::command]
pub fn cron_delete(service: Svc, id: String) -> Result<bool, String> {
    service.cancel_job(&id)
}

#[tauri::command]
pub fn cron_run_now(app: AppHandle, service: Svc, id: String) -> Result<String, String> {
    let run = cron::run_now(&*service.store()?, &id, sessions::now())?;
    let sink = cron_sink(&app, &run.job);
    Arc::clone(&service).cron_dispatch(&run, sink)
}

#[cfg(test)]
#[path = "learning_tests.rs"]
mod learning_tests;
