//! UI-independent application service. Tauri commands are thin wrappers so
//! every behaviour here is testable without a window.

use crate::wire::{envelope, StreamMessage};
use next_agent_core::{
    context_for, run_task, AgentStep, Approval, Approver, EventKind, MockExecutor, SqliteBroker,
    Store, TaskId, TaskSpec, ToolExecutor, ToolRequest, ToolResult, Workspace,
};
use next_agent_permission_engine::Mode;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Sink = Arc<dyn Fn(StreamMessage) + Send + Sync>;

const VAULT_KEY: &str = "vault.path";
/// Settings the UI may read and write. Everything else is rejected.
const SETTING_KEYS: &[&str] = &["router.url", "router.model"];
const DEFAULT_ROUTER_URL: &str = "http://127.0.0.1:20128/v1";
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);

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
}

type Pending = Arc<Mutex<HashMap<String, Sender<Approval>>>>;

pub struct AppService {
    db_path: PathBuf,
    store: Mutex<Store>,
    pending: Pending,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    counter: AtomicU64,
    /// Pause between replayed events so the UI visibly streams.
    pub replay_delay: Duration,
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
            pending: Arc::default(),
            cancels: Mutex::default(),
            counter: AtomicU64::new(0),
            replay_delay: Duration::from_millis(120),
        })
    }

    fn store(&self) -> Result<std::sync::MutexGuard<'_, Store>, String> {
        self.store
            .lock()
            .map_err(|_| "store lock poisoned".to_string())
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
        check_key(key)?;
        let raw = self.store()?.get_setting(key).map_err(|e| e.to_string())?;
        let value = raw.and_then(|r| serde_json::from_str::<String>(&r).ok());
        Ok(value.or_else(|| (key == "router.url").then(|| DEFAULT_ROUTER_URL.to_string())))
    }

    /// Write an allow-listed setting.
    ///
    /// # Errors
    /// Returns a message for unknown keys, bad router URLs or database errors.
    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), String> {
        check_key(key)?;
        // Avoid a double-slash literal: scripts/lib/rust-structure.mjs mistakes it for a comment.
        if key == "router.url" && !is_http_url(value) {
            return Err("router URL must be an http or https URL".into());
        }
        let encoded = serde_json::to_string(value).map_err(|e| e.to_string())?;
        self.store()?
            .set_setting(key, &encoded)
            .map_err(|e| e.to_string())
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

    /// Answer a pending approval card.
    ///
    /// # Errors
    /// Returns a message if the approval is unknown or the choice is invalid.
    pub fn approval_respond(&self, approval_id: &str, choice: &str) -> Result<(), String> {
        let approval = parse_choice(choice)?;
        let sender = self
            .pending
            .lock()
            .map_err(|_| "approvals lock poisoned".to_string())?
            .remove(approval_id)
            .ok_or_else(|| format!("no pending approval {approval_id}"))?;
        sender
            .send(approval)
            .map_err(|_| "task already finished".to_string())
    }

    /// Cancel a running task (denies its pending approvals).
    pub fn task_cancel(&self, task_id: &str) {
        if let Ok(cancels) = self.cancels.lock() {
            if let Some(flag) = cancels.get(task_id) {
                flag.store(true, Ordering::SeqCst);
            }
        }
        if let Ok(mut pending) = self.pending.lock() {
            let prefix = format!("{task_id}:");
            let ids: Vec<String> = pending
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            for id in ids {
                if let Some(tx) = pending.remove(&id) {
                    let _ = tx.send(Approval::Deny);
                }
            }
        }
    }

    /// Create the task row and run it on a background thread, streaming to `sink`.
    ///
    /// # Errors
    /// Returns a message if the workspace/mode is invalid or the task row cannot be created.
    pub fn task_start(
        self: &Arc<Self>,
        request: &TaskRequest,
        sink: Sink,
    ) -> Result<String, String> {
        let prepared = self.prepare(request)?;
        let task_id = prepared.spec.id.as_str().to_string();
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let _ = service.execute(&prepared, &sink);
            if let Ok(mut cancels) = service.cancels.lock() {
                cancels.remove(prepared.spec.id.as_str());
            }
        });
        Ok(task_id)
    }

    fn prepare(&self, request: &TaskRequest) -> Result<Prepared, String> {
        let mode = parse_mode(&request.mode)?;
        let workspace = {
            let store = self.store()?;
            store
                .list_workspaces()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|w| w.id == request.workspace_id)
                .ok_or_else(|| format!("unknown workspace {}", request.workspace_id))?
        };
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = TaskId::from_seed(&format!("{}:{nanos}:{n}", workspace.id));
        let mode_name = mode_name(mode);
        let title: String = request.prompt.chars().take(60).collect();
        self.store()?
            .create_task(
                id.as_str(),
                &workspace.id,
                &title,
                &request.prompt,
                mode_name,
            )
            .map_err(|e| e.to_string())?;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancels
            .lock()
            .map_err(|_| "cancel lock poisoned".to_string())?
            .insert(id.as_str().to_string(), cancelled.clone());
        Ok(Prepared {
            spec: TaskSpec {
                id,
                prompt: request.prompt.clone(),
                mode: mode_name,
                model: "mock-1".into(),
            },
            mode,
            root: PathBuf::from(&workspace.path),
            workspace_id: workspace.id,
            cancelled,
        })
    }

    /// Run a prepared task to completion on the current thread.
    fn execute(&self, prepared: &Prepared, sink: &Sink) -> Result<(), String> {
        let store = Store::open(&self.db_path).map_err(|e| e.to_string())?;
        let task = prepared.spec.id.as_str().to_string();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let context = context_for(prepared.root.clone(), prepared.workspace_id.clone(), home);
        let mut broker = SqliteBroker::new(&store, prepared.mode, context)
            .for_task(task.clone())
            .with_executor(StubExecutor {
                task_id: task.clone(),
                sink: sink.clone(),
            })
            .with_approver(UiApprover {
                task_id: task.clone(),
                sink: sink.clone(),
                pending: self.pending.clone(),
                counter: 0,
            });
        broker.load_rules().map_err(|e| e.to_string())?;
        let mut executor = MockExecutor::new(mock_script(&prepared.root));
        let flag = prepared.cancelled.clone();
        let events = run_task(&mut executor, &mut broker, &prepared.spec, &move || {
            flag.load(Ordering::SeqCst)
        });
        for event in &events {
            let data = crate::wire::kind_data(&event.kind).to_string();
            let _ = store.append_event(&task, event.seq, event.kind.kind(), &data);
            sink(StreamMessage::Event {
                event: envelope(event),
            });
            if !self.replay_delay.is_zero() {
                std::thread::sleep(self.replay_delay);
            }
        }
        Ok(())
    }

    /// Blocking variant used by tests.
    ///
    /// # Errors
    /// Same as [`Self::task_start`] plus execution errors.
    pub fn task_run_blocking(&self, request: &TaskRequest, sink: &Sink) -> Result<String, String> {
        let prepared = self.prepare(request)?;
        self.execute(&prepared, sink)?;
        Ok(prepared.spec.id.as_str().to_string())
    }
}

struct Prepared {
    spec: TaskSpec,
    mode: Mode,
    root: PathBuf,
    workspace_id: String,
    cancelled: Arc<AtomicBool>,
}

fn is_http_url(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(scheme, rest)| matches!(scheme, "http" | "https") && rest.starts_with('/'))
}

fn check_key(key: &str) -> Result<(), String> {
    if SETTING_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(format!("unknown setting `{key}`"))
    }
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

fn parse_choice(choice: &str) -> Result<Approval, String> {
    match choice {
        "allow" => Ok(Approval::Allow),
        "deny" => Ok(Approval::Deny),
        "always" => Ok(Approval::AlwaysForWorkspace),
        other => Err(format!("unknown approval choice `{other}`")),
    }
}

/// Scripted calls for the UI demo: a read, a write (asks in manual) and a command.
fn mock_script(root: &Path) -> Vec<AgentStep> {
    vec![
        AgentStep::Emit(EventKind::Message {
            text: "Looking at the project first.".into(),
        }),
        AgentStep::Call {
            call_id: "call_1".into(),
            request: ToolRequest::Read {
                path: root.join("README.md"),
            },
        },
        AgentStep::Call {
            call_id: "call_2".into(),
            request: ToolRequest::Write {
                path: root.join("NOTES.md"),
                contents: "# Notes\n".into(),
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

/// Executes nothing: the UI demo never touches the filesystem or a shell.
struct StubExecutor {
    task_id: String,
    sink: Sink,
}

impl ToolExecutor for StubExecutor {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult {
        let output = format!(
            "(mock) {} completed; no side effects performed",
            request.kind()
        );
        (self.sink)(StreamMessage::ToolOutput {
            task_id: self.task_id.clone(),
            call_id: call_id.to_string(),
            ok: true,
            output: output.clone(),
        });
        ToolResult::ok(call_id, output)
    }
}

/// Blocks the task until the UI answers via `approval_respond`.
struct UiApprover {
    task_id: String,
    sink: Sink,
    pending: Pending,
    counter: u64,
}

impl Approver for UiApprover {
    fn approve(&mut self, call_id: &str, request: &ToolRequest, reason: &str) -> Approval {
        self.counter += 1;
        let approval_id = format!("{}:{}", self.task_id, self.counter);
        let (tx, rx) = channel();
        match self.pending.lock() {
            Ok(mut pending) => {
                pending.insert(approval_id.clone(), tx);
            }
            Err(_) => return Approval::Deny,
        }
        (self.sink)(StreamMessage::Approval {
            task_id: self.task_id.clone(),
            approval_id: approval_id.clone(),
            call_id: call_id.to_string(),
            tool: request.kind().to_string(),
            summary: summarize(request),
            reason: reason.to_string(),
        });
        // Fail closed on timeout or a dropped sender.
        let choice = rx.recv_timeout(APPROVAL_TIMEOUT).unwrap_or(Approval::Deny);
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&approval_id);
        }
        choice
    }
}

fn summarize(request: &ToolRequest) -> String {
    match request {
        ToolRequest::Read { path } | ToolRequest::ListDir { path } => path.display().to_string(),
        ToolRequest::Write { path, contents } => {
            format!("{} ({} bytes)", path.display(), contents.len())
        }
        ToolRequest::ApplyPatch { path, patch } => {
            format!("{} (patch, {} bytes)", path.display(), patch.len())
        }
        ToolRequest::Search { pattern, path } => format!("{pattern} in {}", path.display()),
        ToolRequest::Exec { command, cwd } => format!("{command}  (in {})", cwd.display()),
        ToolRequest::Network { url } => url.clone(),
        ToolRequest::AskUser { question } => question.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn service() -> (tempfile::TempDir, Arc<AppService>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut svc = AppService::new(dir.path().join("db.sqlite")).expect("service");
        svc.replay_delay = Duration::ZERO;
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

    #[test]
    fn auto_task_streams_ordered_events_and_persists_them() {
        let (dir, svc) = service();
        let ws = dir.path().join("proj");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        let (sink, log) = collector();
        let request = TaskRequest {
            workspace_id: opened.id,
            prompt: "demo".into(),
            mode: "auto".into(),
        };
        let task = svc.task_run_blocking(&request, &sink).expect("run");
        let messages = log.lock().expect("lock");
        let seqs: Vec<u64> = messages
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Event { event } => event["seq"].as_u64(),
                _ => None,
            })
            .collect();
        assert!(seqs.len() >= 4, "{seqs:?}");
        assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
        let persisted = svc
            .store()
            .expect("store")
            .event_kinds(&task)
            .expect("kinds");
        assert_eq!(persisted.len(), seqs.len());
        assert_eq!(persisted[0], "task_started");
        assert_eq!(persisted.last().map(String::as_str), Some("task_completed"));
    }

    #[test]
    fn manual_task_blocks_on_approval_until_answered() {
        let (dir, svc) = service();
        let ws = dir.path().join("proj");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        let (inner, log) = collector();
        let responder = Arc::clone(&svc);
        let sink: Sink = Arc::new(move |m| {
            if let StreamMessage::Approval { approval_id, .. } = &m {
                responder
                    .approval_respond(approval_id, "deny")
                    .expect("respond");
            }
            inner(m);
        });
        let request = TaskRequest {
            workspace_id: opened.id,
            prompt: "demo".into(),
            mode: "manual".into(),
        };
        svc.task_run_blocking(&request, &sink).expect("run");
        let messages = log.lock().expect("lock");
        assert!(messages
            .iter()
            .any(|m| matches!(m, StreamMessage::Approval { tool, .. } if tool == "write_file")));
        let denied = messages.iter().any(|m| match m {
            // The policy decision is `ask`; the user's deny is recorded as
            // the approval resolution (core event contract).
            StreamMessage::Event { event } => {
                event["kind"] == "approval_resolved" && event["data"]["choice"] == "deny"
            }
            _ => false,
        });
        assert!(denied, "denied write must be recorded");
    }

    #[test]
    fn approval_respond_rejects_unknown_ids_and_choices() {
        let (_dir, svc) = service();
        assert!(svc.approval_respond("nope", "allow").is_err());
        assert!(svc.approval_respond("nope", "maybe").is_err());
    }

    #[test]
    fn task_start_rejects_unknown_workspace_and_mode() {
        let (_dir, svc) = service();
        let (sink, _log) = collector();
        let bad_ws = TaskRequest {
            workspace_id: "ws_missing".into(),
            prompt: "x".into(),
            mode: "auto".into(),
        };
        assert!(svc.task_start(&bad_ws, sink.clone()).is_err());
        let bad_mode = TaskRequest {
            workspace_id: "ws_missing".into(),
            prompt: "x".into(),
            mode: "yolo".into(),
        };
        assert!(svc.task_start(&bad_mode, sink).is_err());
    }

    #[test]
    fn audit_chain_of_fresh_db_verifies() {
        let (_dir, svc) = service();
        assert!(svc.audit_verify().expect("verify").ok);
    }
}
