//! UI-independent application service. Tauri commands are thin wrappers so
//! every behaviour here is testable without a window.

use crate::tasks::FinishedTask;
use crate::wire::StreamMessage;
use next_agent_core::router::settings::normalize_base_url;
use next_agent_core::{Store, StoredWorkspace, TaskControl, Workspace};
use next_agent_permission_engine::Mode;
use next_agent_process_manager::HealthReport;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use tokio::sync::watch;

pub type Sink = Arc<dyn Fn(StreamMessage) + Send + Sync>;

const VAULT_KEY: &str = "vault.path";
/// UI setting names mapped to the core's settings keys. Everything else is rejected.
const SETTING_KEYS: &[(&str, &str)] = &[
    ("router.url", "router.base_url"),
    ("router.model", "router.default_model"),
];
const DEFAULT_ROUTER_URL: &str = "http://127.0.0.1:20128/v1";

/// Router status for the status-bar dot.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthDto {
    pub status: String,
    pub detail: String,
    pub latency_ms: Option<u64>,
    pub checks: u64,
}

impl From<&HealthReport> for HealthDto {
    fn from(report: &HealthReport) -> Self {
        Self {
            status: report.status.as_str().to_string(),
            detail: report.detail.clone(),
            latency_ms: report.latency_ms,
            checks: report.checks,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceDto {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuleDto {
    pub id: String,
    pub scope: String,
    pub effect: String,
    pub kind: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuditStatus {
    pub ok: bool,
    pub checked: usize,
    pub broken_at: Option<i64>,
}

/// Parameters of `task_start`.
#[derive(Debug, Clone)]
pub struct TaskRequest {
    pub workspace_id: String,
    pub prompt: String,
    pub mode: String,
    /// Agent profile to run as (`None` = the mode's default: build / plan).
    pub profile: Option<String>,
}

fn lock<'a, T>(mutex: &'a Mutex<T>, what: &str) -> Result<MutexGuard<'a, T>, String> {
    mutex.lock().map_err(|_| format!("{what} lock poisoned"))
}

pub struct AppService {
    db_path: PathBuf,
    store: Mutex<Store>,
    counter: AtomicU64,
    running: Mutex<HashMap<String, Arc<TaskControl>>>,
    joins: Mutex<HashMap<String, JoinHandle<()>>>,
    finished: Mutex<HashMap<String, FinishedTask>>,
    health: Mutex<Option<HealthDto>>,
    /// Bumped when router settings change so the health poller rebuilds its client.
    router_generation: watch::Sender<u64>,
}

impl AppService {
    /// Open (or create) the database at `db_path`.
    ///
    /// # Errors
    /// Returns a message if the database cannot be opened.
    pub fn new(db_path: impl Into<PathBuf>) -> Result<Self, String> {
        let db_path = db_path.into();
        let store = Store::open(&db_path).map_err(|e| e.to_string())?;
        Ok(Self {
            db_path,
            store: Mutex::new(store),
            counter: AtomicU64::new(0),
            running: Mutex::default(),
            joins: Mutex::default(),
            finished: Mutex::default(),
            health: Mutex::default(),
            router_generation: watch::channel(0).0,
        })
    }

    pub(crate) fn store(&self) -> Result<MutexGuard<'_, Store>, String> {
        lock(&self.store, "store")
    }
    /// Open a fresh connection to the database at `db_path`.
    pub(crate) fn open_store(&self) -> Result<Store, String> {
        Store::open(self.db_path()).map_err(|e| e.to_string())
    }
    pub(crate) fn db_path(&self) -> &Path {
        &self.db_path
    }
    pub(crate) fn counter(&self) -> &AtomicU64 {
        &self.counter
    }
    pub(crate) fn running(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<String, Arc<TaskControl>>>, String> {
        lock(&self.running, "tasks")
    }
    pub(crate) fn joins(&self) -> Result<MutexGuard<'_, HashMap<String, JoinHandle<()>>>, String> {
        lock(&self.joins, "joins")
    }
    pub(crate) fn finished(&self) -> Result<MutexGuard<'_, HashMap<String, FinishedTask>>, String> {
        lock(&self.finished, "finished tasks")
    }

    /// Latest router health, if probed yet.
    #[must_use]
    pub fn health(&self) -> Option<HealthDto> {
        self.health.lock().ok().and_then(|h| h.clone())
    }

    /// Record a router health report (called by the poller).
    pub fn set_health(&self, report: &HealthReport) {
        if let Ok(mut health) = self.health.lock() {
            *health = Some(HealthDto::from(report));
        }
    }

    /// Tell the health poller that provider settings changed.
    pub(crate) fn bump_router_generation(&self) {
        self.router_generation.send_modify(|g| *g += 1);
    }

    /// Changes whenever router settings change.
    #[must_use]
    pub fn router_generation(&self) -> watch::Receiver<u64> {
        self.router_generation.subscribe()
    }

    pub(crate) fn workspace(&self, id: &str) -> Result<StoredWorkspace, String> {
        self.store()?
            .list_workspaces()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| format!("unknown workspace {id}"))
    }

    /// Register (or re-open) a folder as a workspace.
    ///
    /// # Errors
    /// Returns a message for missing/non-directory paths or database errors.
    pub fn workspace_open(&self, path: &str) -> Result<WorkspaceDto, String> {
        let workspace = Workspace::open(path).map_err(|e| e.to_string())?;
        self.store()?
            .upsert_workspace(&workspace)
            .map_err(|e| e.to_string())?;
        Ok(WorkspaceDto {
            id: workspace.id().to_string(),
            path: workspace.root().display().to_string(),
            name: workspace.name().to_string(),
        })
    }

    /// Recent workspaces.
    ///
    /// # Errors
    /// Returns a message on database errors.
    pub fn workspace_list(&self) -> Result<Vec<WorkspaceDto>, String> {
        let rows = self.store()?.list_workspaces().map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|w| WorkspaceDto {
                id: w.id,
                path: w.path,
                name: w.name,
            })
            .collect())
    }

    /// Point the app at a docs vault folder (must exist). Returns the canonical path.
    ///
    /// # Errors
    /// Returns a message if the path is not an existing directory.
    pub fn vault_set(&self, path: &str) -> Result<String, String> {
        let canonical = Path::new(path)
            .canonicalize()
            .map_err(|_| format!("vault path does not exist: {path}"))?;
        if !canonical.is_dir() {
            return Err(format!("vault path is not a directory: {path}"));
        }
        let text = canonical.display().to_string();
        let encoded = serde_json::to_string(&text).map_err(|e| e.to_string())?;
        self.store()?
            .set_setting(VAULT_KEY, &encoded)
            .map_err(|e| e.to_string())?;
        Ok(text)
    }

    /// Currently configured vault path.
    ///
    /// # Errors
    /// Returns a message on database errors.
    pub fn vault_get(&self) -> Result<Option<String>, String> {
        let raw = self
            .store()?
            .get_setting(VAULT_KEY)
            .map_err(|e| e.to_string())?;
        Ok(raw.and_then(|r| serde_json::from_str::<String>(&r).ok()))
    }

    /// Read an allow-listed setting (router URL has a default).
    ///
    /// # Errors
    /// Returns a message for unknown keys or database errors.
    pub fn setting_get(&self, key: &str) -> Result<Option<String>, String> {
        let stored = core_key(key)?;
        let raw = self
            .store()?
            .get_setting(stored)
            .map_err(|e| e.to_string())?;
        let value = raw
            .and_then(|r| serde_json::from_str::<String>(&r).ok())
            .filter(|v| !v.is_empty());
        Ok(value.or_else(|| (key == "router.url").then(|| DEFAULT_ROUTER_URL.to_string())))
    }

    /// Write an allow-listed setting.
    ///
    /// # Errors
    /// Returns a message for unknown keys, bad router URLs or database errors.
    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), String> {
        let stored = core_key(key)?;
        let value = if key == "router.url" {
            normalize_base_url(value)
                .map_err(|_| "router URL must be an http or https URL".to_string())?
        } else {
            value.trim().to_string()
        };
        let encoded = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        self.store()?
            .set_setting(stored, &encoded)
            .map_err(|e| e.to_string())?;
        self.router_generation.send_modify(|g| *g += 1);
        Ok(())
    }

    /// Permission rules visible in a workspace (global + its own).
    ///
    /// # Errors
    /// Returns a message on database errors.
    pub fn rules_list(&self, workspace_id: &str) -> Result<Vec<RuleDto>, String> {
        let rows = self
            .store()?
            .rules_for(workspace_id)
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| RuleDto {
                id: r.id,
                scope: r.scope,
                effect: r.effect,
                kind: r.kind,
                pattern: r.pattern,
            })
            .collect())
    }

    /// Revoke a rule.
    ///
    /// # Errors
    /// Returns a message on database errors.
    pub fn rule_delete(&self, id: &str) -> Result<bool, String> {
        self.store()?.delete_rule(id).map_err(|e| e.to_string())
    }

    /// Verify the audit hash chain (the list view needs a core query API).
    ///
    /// # Errors
    /// Returns a message on database errors.
    pub fn audit_verify(&self) -> Result<AuditStatus, String> {
        let v = self.store()?.verify_audit().map_err(|e| e.to_string())?;
        Ok(AuditStatus {
            ok: v.ok,
            checked: v.checked,
            broken_at: v.broken_at,
        })
    }
}

fn core_key(key: &str) -> Result<&'static str, String> {
    SETTING_KEYS
        .iter()
        .find(|(ui, _)| *ui == key)
        .map(|(_, core)| *core)
        .ok_or_else(|| format!("unknown setting `{key}`"))
}

/// Parse the UI's mode string.
///
/// # Errors
/// Returns a message for unknown names.
pub fn parse_mode(name: &str) -> Result<Mode, String> {
    match name {
        "plan" => Ok(Mode::Plan),
        "manual" => Ok(Mode::Manual),
        "auto" => Ok(Mode::Auto),
        "bypassPermissions" => Ok(Mode::BypassPermissions),
        other => Err(format!("unknown mode `{other}`")),
    }
}

#[must_use]
pub fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Plan => "plan",
        Mode::Manual => "manual",
        Mode::Auto => "auto",
        Mode::BypassPermissions => "bypassPermissions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn service() -> (tempfile::TempDir, Arc<AppService>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = AppService::new(dir.path().join("db.sqlite")).expect("service");
        (dir, Arc::new(svc))
    }

    fn collector() -> (Sink, Arc<StdMutex<Vec<StreamMessage>>>) {
        let log: Arc<StdMutex<Vec<StreamMessage>>> = Arc::default();
        let inner = log.clone();
        let sink: Sink = Arc::new(move |m| inner.lock().expect("lock").push(m));
        (sink, log)
    }

    #[test]
    fn workspace_open_persists_and_lists() {
        let (dir, svc) = service();
        let ws = dir.path().join("proj");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        assert_eq!(opened.name, "proj");
        let again = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("reopen");
        assert_eq!(opened.id, again.id);
        assert_eq!(svc.workspace_list().expect("list").len(), 1);
        // survives a restart
        let restarted = AppService::new(dir.path().join("db.sqlite")).expect("restart");
        assert_eq!(restarted.workspace_list().expect("list")[0].id, opened.id);
    }

    #[test]
    fn workspace_open_rejects_bad_paths() {
        let (dir, svc) = service();
        assert!(svc.workspace_open("").is_err());
        assert!(svc.workspace_open("/definitely/not/here").is_err());
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").expect("write");
        assert!(svc.workspace_open(file.to_str().expect("utf8")).is_err());
    }

    #[test]
    fn vault_set_validates_and_round_trips() {
        let (dir, svc) = service();
        assert_eq!(svc.vault_get().expect("get"), None);
        assert!(svc.vault_set("/no/such/vault").is_err());
        let canonical = svc
            .vault_set(dir.path().to_str().expect("utf8"))
            .expect("set");
        assert_eq!(svc.vault_get().expect("get"), Some(canonical));
    }

    #[test]
    fn settings_are_allow_listed() {
        let (_dir, svc) = service();
        assert_eq!(
            svc.setting_get("router.url").expect("default").as_deref(),
            Some(DEFAULT_ROUTER_URL)
        );
        assert!(svc.setting_get("vault.path").is_err());
        assert!(svc.setting_set("router.url", "ftp://x").is_err());
        svc.setting_set("router.url", "http://localhost:1/v1")
            .expect("set");
        assert_eq!(
            svc.setting_get("router.url").expect("get").as_deref(),
            Some("http://localhost:1/v1")
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }

    /// A workspace that is a git repo with one commit, plus a vault folder.
    fn repo_workspace(dir: &Path, svc: &AppService) -> (PathBuf, String) {
        let ws = dir.join("proj");
        std::fs::create_dir(&ws).expect("mkdir");
        git(&ws, &["init", "-q", "-b", "main"]);
        git(&ws, &["config", "user.email", "test@example.invalid"]);
        git(&ws, &["config", "user.name", "Test"]);
        std::fs::write(ws.join("README.md"), "# proj\n").expect("write");
        git(&ws, &["add", "."]);
        git(&ws, &["commit", "-q", "-m", "init"]);
        let vault = dir.join("vault");
        std::fs::create_dir(&vault).expect("vault");
        svc.vault_set(vault.to_str().expect("utf8"))
            .expect("vault set");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        (PathBuf::from(opened.path), opened.id)
    }

    fn run(svc: &Arc<AppService>, workspace_id: &str, mode: &str, sink: Sink) -> String {
        let request = TaskRequest {
            workspace_id: workspace_id.into(),
            prompt: "Add a demo note".into(),
            mode: mode.into(),
            profile: None,
        };
        let id = svc.task_start(&request, sink).expect("start");
        svc.task_wait(&id);
        id
    }

    fn events_of(messages: &[StreamMessage]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Event { event } => Some(event.clone()),
                _ => None,
            })
            .collect()
    }

    /// The M1 exit scenario without a window: auto task in a worktree,
    /// review diff, task record in the vault, merge into the main tree.
    #[test]
    fn auto_task_runs_in_worktree_then_merges_and_documents() {
        let (dir, svc) = service();
        let (root, ws_id) = repo_workspace(dir.path(), &svc);
        let (sink, log) = collector();
        let task = run(&svc, &ws_id, "auto", sink);
        let messages = log.lock().expect("lock").clone();

        let events = events_of(&messages);
        let seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
        assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
        assert_eq!(events.last().expect("events")["kind"], "task_completed");
        let persisted = svc
            .store()
            .expect("store")
            .event_kinds(&task)
            .expect("kinds");
        assert_eq!(persisted.len(), events.len());

        let branch = messages
            .iter()
            .find_map(|m| match m {
                StreamMessage::Worktree { branch, .. } => Some(branch.clone()),
                _ => None,
            })
            .expect("worktree message");
        assert!(branch.starts_with("next-agent/auto-"), "{branch}");
        assert!(
            !root.join("NEXT_AGENT_DEMO.md").exists(),
            "main tree untouched before merge"
        );

        let files = messages
            .iter()
            .find_map(|m| match m {
                StreamMessage::Review { files, .. } => Some(files.clone()),
                _ => None,
            })
            .expect("review message");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "NEXT_AGENT_DEMO.md");

        let record = messages
            .iter()
            .find_map(|m| match m {
                StreamMessage::Documented { path, .. } => Some(PathBuf::from(path)),
                _ => None,
            })
            .expect("documented message");
        let text = std::fs::read_to_string(&record).expect("record");
        assert!(text.contains("NEXT_AGENT_DEMO.md"), "{text}");

        let outcome = svc.review_action(&task, "merge").expect("merge");
        assert!(outcome.commit.is_some());
        assert!(
            root.join("NEXT_AGENT_DEMO.md").exists(),
            "merged into main tree"
        );
        let text = std::fs::read_to_string(&record).expect("record");
        assert!(text.contains("merged: true"), "{text}");
        assert!(
            svc.review_action(&task, "merge").is_err(),
            "worktree is gone after merge"
        );
    }

    #[test]
    fn undo_last_step_restores_the_task_worktree() {
        let (dir, svc) = service();
        let (_root, ws_id) = repo_workspace(dir.path(), &svc);
        let (sink, log) = collector();
        let task = run(&svc, &ws_id, "auto", sink);
        let wt_root = log
            .lock()
            .expect("lock")
            .iter()
            .find_map(|m| match m {
                StreamMessage::Worktree { root, .. } => Some(PathBuf::from(root)),
                _ => None,
            })
            .expect("worktree");
        let note = wt_root.join("NEXT_AGENT_DEMO.md");
        assert!(note.exists(), "the demo agent wrote its note");
        assert!(svc.task_undoable(&task).expect("count") >= 1);

        // Undo until the write is reverted (the demo runs list, write, ls).
        while note.exists() {
            svc.task_undo_step(&task).expect("undo");
        }
        assert!(svc.task_undo_step(&task).is_err() || !note.exists());
    }

    #[test]
    fn profiles_list_the_primary_built_ins() {
        let (dir, svc) = service();
        let (_root, ws_id) = repo_workspace(dir.path(), &svc);
        let list = svc.agent_profiles(&ws_id).expect("profiles");
        let names: Vec<&str> = list.profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(
            names.contains(&"build") && names.contains(&"plan"),
            "{names:?}"
        );
        assert!(
            !names.contains(&"explore"),
            "subagent-only profiles are hidden"
        );
    }

    #[test]
    fn discard_drops_the_worktree_and_leaves_main_tree_alone() {
        let (dir, svc) = service();
        let (root, ws_id) = repo_workspace(dir.path(), &svc);
        let (sink, _log) = collector();
        let task = run(&svc, &ws_id, "auto", sink);
        svc.review_action(&task, "discard").expect("discard");
        assert!(!root.join("NEXT_AGENT_DEMO.md").exists());
        assert!(!root
            .join(".next-agent/worktrees")
            .read_dir()
            .is_ok_and(|mut d| d.next().is_some()));
    }

    /// The merge stands even when the task record cannot be updated, but the
    /// UI is told the record is stale.
    #[test]
    fn merge_reports_a_task_record_that_could_not_be_updated() {
        let (dir, svc) = service();
        let (root, ws_id) = repo_workspace(dir.path(), &svc);
        let (sink, _log) = collector();
        let task = run(&svc, &ws_id, "auto", sink);
        // Make the vault unwritable: a file where the folder was.
        let vault = dir.path().join("vault");
        std::fs::remove_dir_all(&vault).expect("rm vault");
        std::fs::write(&vault, "not a folder").expect("block vault");

        let outcome = svc.review_action(&task, "merge").expect("merge");
        assert!(outcome.commit.is_some());
        assert!(root.join("NEXT_AGENT_DEMO.md").exists(), "merged");
        let notice = outcome.notice.expect("a notice about the record");
        assert!(notice.contains("task record"), "{notice}");
    }

    /// When step snapshots cannot be opened the task still runs, and the
    /// timeline says per-step undo is off.
    #[test]
    fn a_task_without_step_snapshots_says_undo_is_off() {
        let (dir, svc) = service();
        let (_root, ws_id) = repo_workspace(dir.path(), &svc);
        // A file where the snapshot folder would go.
        std::fs::write(dir.path().join("snapshot"), "blocked").expect("block");
        let (sink, log) = collector();
        let task = run(&svc, &ws_id, "auto", sink);
        let notices: Vec<String> = log
            .lock()
            .expect("lock")
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Notice { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            notices.iter().any(|n| n.contains("Undo is off")),
            "{notices:?}"
        );
        assert_eq!(svc.task_undoable(&task).expect("count"), 0);
    }

    #[test]
    fn auto_on_non_git_folder_falls_back_to_manual_and_asks() {
        let (dir, svc) = service();
        let ws = dir.path().join("plain");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        let (inner, log) = collector();
        let responder = Arc::clone(&svc);
        let sink: Sink = Arc::new(move |m| {
            if let StreamMessage::Event { event } = &m {
                if event["kind"] == "approval_requested" {
                    let task = event["task_id"].as_str().expect("task");
                    let approval = event["data"]["approval_id"].as_str().expect("id");
                    responder
                        .approval_respond(task, approval, "deny")
                        .expect("respond");
                }
            }
            inner(m);
        });
        run(&svc, &opened.id, "auto", sink);
        let messages = log.lock().expect("lock").clone();
        assert!(messages.iter().any(
            |m| matches!(m, StreamMessage::Notice { text, .. } if text.contains("runs in Manual mode"))
        ));
        let events = events_of(&messages);
        assert_eq!(events[0]["data"]["mode"], "manual");
        assert!(events
            .iter()
            .any(|e| e["kind"] == "approval_resolved" && e["data"]["choice"] == "deny"));
        assert!(
            !ws.join("NEXT_AGENT_DEMO.md").exists(),
            "denied write never happened"
        );
    }

    #[test]
    fn approval_respond_rejects_unknown_tasks_and_choices() {
        let (_dir, svc) = service();
        assert!(svc.approval_respond("t", "nope", "allow").is_err());
        assert!(svc.approval_respond("t", "nope", "maybe").is_err());
    }

    #[test]
    fn task_start_rejects_unknown_workspace_and_mode() {
        let (_dir, svc) = service();
        let (sink, _log) = collector();
        let bad_ws = TaskRequest {
            workspace_id: "ws_missing".into(),
            prompt: "x".into(),
            mode: "auto".into(),
            profile: None,
        };
        assert!(svc.task_start(&bad_ws, sink.clone()).is_err());
        let bad_mode = TaskRequest {
            workspace_id: "ws_missing".into(),
            prompt: "x".into(),
            mode: "yolo".into(),
            profile: None,
        };
        assert!(svc.task_start(&bad_mode, sink).is_err());
    }

    #[test]
    fn audit_chain_of_fresh_db_verifies() {
        let (_dir, svc) = service();
        assert!(svc.audit_verify().expect("verify").ok);
    }
}
