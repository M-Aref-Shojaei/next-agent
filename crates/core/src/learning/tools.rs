//! The learning tools as broker requests, and the executor for them.
//!
//! [`Learning::request`] turns a model call into a [`ToolRequest::Learn`]
//! whose `target` is what the permission engine judges:
//! - `memory`: the `MEMORY.md` / `USER.md` file it writes (a write);
//! - `skill_manage`: the skills folder it writes (a write; one folder per
//!   call, so one approval covers exactly what changes);
//! - `skills_list`, `skill_view`, `session_search`: the workspace's
//!   `.next-agent` state (a read).
//!
//! [`LearningTools`] runs calls the broker allowed and hands everything
//! else to the wrapped executor. It never re-checks policy.

use crate::broker::ToolExecutor;
use crate::learning::memory::{self, MemoryOp, MemoryStore, Target};
use crate::learning::sessions::{self, SearchScope};
use crate::learning::skills::{self, ChangeKind, ManageContext, Scope, SkillOp, Skills};
use crate::runner::RunContext;
use crate::store::{SkillCounter, Store};
use crate::tools::{LearnRequest, ToolRequest, ToolResult};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// Agent-facing names of the learning tools.
pub const TOOL_NAMES: [&str; 5] = [
    "memory",
    "skills_list",
    "skill_view",
    "skill_manage",
    "session_search",
];

/// Which learning tools a task gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSet {
    pub memory: bool,
    pub skills: bool,
    pub session_search: bool,
}

impl ToolSet {
    pub const ALL: Self = Self {
        memory: true,
        skills: true,
        session_search: true,
    };

    #[must_use]
    pub fn offers(self, name: &str) -> bool {
        match name {
            "memory" => self.memory,
            "skills_list" | "skill_view" | "skill_manage" => self.skills,
            "session_search" => self.session_search,
            _ => false,
        }
    }
}

/// The policy kind of a learning tool (`write_file` for the writers).
#[must_use]
pub fn policy_kind(name: &str) -> &'static str {
    match name {
        "memory" | "skill_manage" => "write_file",
        _ => "read_file",
    }
}

/// One write a learning tool made (for the timeline notice and audit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnChange {
    pub tool: &'static str,
    pub summary: String,
}

/// Everything the learning tools of one task need.
#[derive(Debug, Clone)]
pub struct Learning {
    pub workspace_id: String,
    /// The task's sandbox root (the worktree in auto mode). Reads of the
    /// learning state are judged as reads of its `.next-agent` folder.
    pub sandbox_root: PathBuf,
    /// This workspace's `MEMORY.md` / `USER.md` folder.
    pub memory_dir: PathBuf,
    pub skills: Skills,
    /// The metadata database (session store, skill telemetry).
    pub db_path: PathBuf,
    /// This task's session (hidden from its own searches).
    pub session_id: Option<String>,
    pub tools: ToolSet,
    /// Background-review restrictions: memory add-only, no skill deletes,
    /// read-before-write, content scanned.
    pub review: bool,
}

fn tool_name(name: &str) -> Option<&'static str> {
    TOOL_NAMES.iter().copied().find(|n| *n == name)
}

impl Learning {
    #[must_use]
    pub fn memory(&self) -> MemoryStore {
        MemoryStore::new(&self.memory_dir)
    }

    /// Schemas for the offered tools.
    #[must_use]
    pub fn schemas(&self) -> Vec<Value> {
        let mut out = Vec::new();
        if self.tools.memory {
            out.push(memory::tool_schema());
        }
        if self.tools.skills {
            out.extend(skills::tool_schemas());
        }
        if self.tools.session_search {
            out.push(sessions::tool_schema());
        }
        out
    }

    /// The frozen context a task starts with: the memory snapshot and the
    /// skills index (`None` when there is nothing to show).
    #[must_use]
    pub fn preamble(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.tools.memory {
            parts.extend(memory::render_snapshot(&self.memory().snapshot()));
        }
        if self.tools.skills {
            parts.extend(self.skills.index_text());
        }
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }

    /// Static guidance for the system prompt (no per-workspace data).
    #[must_use]
    pub fn guidance(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.tools.memory {
            parts.push(memory::MEMORY_GUIDANCE.to_string());
        }
        if self.tools.skills {
            parts.push("Skills are reusable procedures in SKILL.md files. When a skill in the index fits the task, load it with skill_view before you start and follow it; after a task that taught you something reusable, update the skill (skill_manage patch) or create one.".into());
        }
        if self.tools.session_search {
            parts.push("Use session_search to look up what was done in earlier tasks in this workspace instead of guessing.".into());
        }
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }

    /// Turn a model call into a broker request, or `None` when `name` is
    /// not a learning tool this task offers.
    ///
    /// # Errors
    /// Malformed arguments, or a `skill_manage` call spanning two folders.
    pub fn request(&self, name: &str, args: &Value) -> Option<Result<ToolRequest, String>> {
        let tool = tool_name(name).filter(|n| self.tools.offers(n))?;
        let state = self.sandbox_root.join(".next-agent");
        let learn = |target: PathBuf, mutating: bool| {
            Ok(ToolRequest::Learn(LearnRequest {
                tool,
                args: args.clone(),
                target,
                mutating,
            }))
        };
        Some(match tool {
            "memory" => {
                let target = args
                    .get("target")
                    .and_then(Value::as_str)
                    .and_then(Target::parse)
                    .unwrap_or(Target::Memory);
                learn(self.memory().path(target), true)
            }
            "skill_manage" => match self.manage_root(args) {
                Ok(root) => learn(root, true),
                Err(error) => Err(error),
            },
            _ => learn(state, false),
        })
    }

    /// The one skills folder a `skill_manage` call writes.
    fn manage_root(&self, args: &Value) -> Result<PathBuf, String> {
        let items = args
            .get("operations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let workspace = self.skills.root(Scope::Workspace).map(|r| r.path.clone());
        let mut roots: Vec<PathBuf> = Vec::new();
        for item in &items {
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
            let name = name.rsplit('/').next().unwrap_or_default();
            let root = if item.get("action").and_then(Value::as_str) == Some("create") {
                let scope = item
                    .get("scope")
                    .and_then(Value::as_str)
                    .and_then(Scope::parse)
                    .unwrap_or(Scope::Workspace);
                self.skills.root(scope).map(|r| r.path.clone())
            } else {
                self.skills
                    .find(name)
                    .map(|s| s.root)
                    .or_else(|| workspace.clone())
            };
            if let Some(root) = root {
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
        match roots.len() {
            0 => workspace.ok_or_else(|| "no skills folder is configured".into()),
            1 => Ok(roots.remove(0)),
            _ => Err("one skill_manage call may change skills in one folder only (workspace or user); split the operations into two calls.".into()),
        }
    }
}

/// Executes learning calls; everything else goes to `inner`.
pub struct LearningTools {
    inner: Box<dyn ToolExecutor + Send>,
    learning: Learning,
    store: Option<Store>,
    viewed: HashSet<(String, String)>,
    memory_failures: usize,
    changes: Arc<Mutex<Vec<LearnChange>>>,
}

impl std::fmt::Debug for LearningTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LearningTools")
            .field("learning", &self.learning)
            .finish_non_exhaustive()
    }
}

fn result(call_id: &str, value: &Value) -> ToolResult {
    let ok = value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = serde_json::to_string(value).unwrap_or_default();
    if ok {
        ToolResult::ok(call_id, text)
    } else {
        ToolResult::error(call_id, text)
    }
}

impl LearningTools {
    #[must_use]
    pub fn new(inner: Box<dyn ToolExecutor + Send>, learning: Learning) -> Self {
        Self {
            inner,
            learning,
            store: None,
            viewed: HashSet::new(),
            memory_failures: 0,
            changes: Arc::default(),
        }
    }

    /// Writes made so far (shared, so the caller can read them after the
    /// task has consumed the executor).
    #[must_use]
    pub fn changes(&self) -> Arc<Mutex<Vec<LearnChange>>> {
        Arc::clone(&self.changes)
    }

    fn store(&mut self) -> Result<&Store, String> {
        if self.store.is_none() {
            self.store = Some(Store::open(&self.learning.db_path).map_err(|e| e.to_string())?);
        }
        self.store
            .as_ref()
            .ok_or_else(|| "store unavailable".into())
    }

    fn note(&self, tool: &'static str, summary: String) {
        self.changes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(LearnChange { tool, summary });
    }

    fn run(&mut self, learn: &LearnRequest) -> Value {
        let fail = |message: String| serde_json::json!({"success": false, "error": message});
        match learn.tool {
            "memory" => self.memory(&learn.args),
            "skills_list" => self
                .learning
                .skills
                .list_json(learn.args.get("category").and_then(Value::as_str)),
            "skill_view" => {
                let name = learn
                    .args
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file = learn
                    .args
                    .get("file_path")
                    .and_then(Value::as_str)
                    .filter(|f| !f.is_empty());
                match self.learning.skills.view_json(name, file) {
                    Ok(value) => {
                        let name = value["name"].as_str().unwrap_or(name).to_string();
                        self.viewed
                            .insert((name.clone(), file.unwrap_or("SKILL.md").to_string()));
                        if let Some(skill) = self.learning.skills.find(&name) {
                            let root = skill.root.display().to_string();
                            if let Ok(store) = self.store() {
                                let _ = store.skill_bump(
                                    &root,
                                    &name,
                                    SkillCounter::View,
                                    sessions::now(),
                                );
                            }
                        }
                        value
                    }
                    Err(error) => fail(error),
                }
            }
            "skill_manage" => self.skill_manage(&learn.args),
            "session_search" => {
                let workspace = self.learning.workspace_id.clone();
                let session = self.learning.session_id.clone();
                match self.store() {
                    Ok(store) => sessions::session_search(
                        store,
                        &SearchScope {
                            workspace_id: &workspace,
                            current_session: session.as_deref(),
                        },
                        &learn.args,
                    ),
                    Err(error) => fail(error),
                }
            }
            other => fail(format!("unknown learning tool `{other}`")),
        }
    }

    fn memory(&mut self, args: &Value) -> Value {
        if self.memory_failures > memory::MAX_CONSECUTIVE_FAILURES {
            return serde_json::json!({"success": false, "done": true, "error": format!(
                "Memory consolidation failed {} times this task. Stop retrying memory calls — leave memory unchanged for now and continue with your reply to the user. The fact can be saved in a later task.",
                self.memory_failures
            )});
        }
        let parsed = (|| -> Result<(Target, Vec<MemoryOp>), String> {
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .and_then(Target::parse)
                .ok_or("target must be 'memory' or 'user'.")?;
            let ops: Vec<MemoryOp> = match args.get("operations").and_then(Value::as_array) {
                Some(items) => items
                    .iter()
                    .map(MemoryOp::from_json)
                    .collect::<Result<_, _>>()?,
                // Legacy single-op shape.
                None => vec![MemoryOp::from_json(args)?],
            };
            Ok((target, ops))
        })();
        let value = match parsed {
            Ok((target, ops)) => self
                .learning
                .memory()
                .apply(target, &ops, self.learning.review),
            Err(error) => serde_json::json!({"success": false, "error": error}),
        };
        if value["success"] == true {
            self.memory_failures = 0;
            let target = value["target"].as_str().unwrap_or("memory");
            let label = if target == "user" {
                "User profile updated"
            } else {
                "Memory updated"
            };
            if value["message"] != "Entry already exists (no duplicate added)." {
                self.note(
                    "memory",
                    format!("{label}: {}", value["message"].as_str().unwrap_or_default()),
                );
            }
        } else {
            self.memory_failures += 1;
        }
        value
    }

    fn skill_manage(&mut self, args: &Value) -> Value {
        let items = args
            .get("operations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| vec![args.clone()]);
        let ops: Result<Vec<SkillOp>, String> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                SkillOp::from_json(item).map_err(|e| {
                    if items.len() > 1 {
                        format!("operations[{i}]: {e}")
                    } else {
                        e
                    }
                })
            })
            .collect();
        let ops = match ops {
            Ok(ops) => ops,
            Err(error) => return serde_json::json!({"success": false, "error": error}),
        };
        let roots: Vec<String> = self
            .learning
            .skills
            .roots()
            .iter()
            .map(|r| r.path.display().to_string())
            .collect();
        let pinned: Vec<String> = match self.store() {
            Ok(store) => roots
                .iter()
                .flat_map(|root| store.skill_usage(root).unwrap_or_default())
                .filter(|row| row.pinned)
                .map(|row| row.name)
                .collect(),
            Err(_) => Vec::new(),
        };
        let cx = ManageContext {
            review: self.learning.review,
            viewed: Some(&self.viewed),
            pinned: &pinned,
        };
        let outcome = self.learning.skills.apply_batch(&ops, &cx);
        let now = sessions::now();
        for change in &outcome.changes {
            let root = change.root.display().to_string();
            let verb = match change.kind {
                ChangeKind::Created => "created",
                ChangeKind::Patched => "patched",
                ChangeKind::Archived => "archived",
            };
            if let Ok(store) = self.store() {
                let _ = match change.kind {
                    ChangeKind::Created => store.skill_created(&root, &change.name, "agent", now),
                    ChangeKind::Patched => {
                        store.skill_bump(&root, &change.name, SkillCounter::Patch, now)
                    }
                    ChangeKind::Archived => {
                        store.skill_set_state(&root, &change.name, "archived", now)
                    }
                };
            }
            self.note("skill_manage", format!("Skill '{}' {verb}", change.name));
        }
        outcome.result
    }
}

impl ToolExecutor for LearningTools {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult {
        match request {
            ToolRequest::Learn(learn) => {
                let value = self.run(learn);
                result(call_id, &value)
            }
            other => self.inner.execute(call_id, other),
        }
    }

    fn execute_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> ToolResult {
        match request {
            ToolRequest::Learn(_) => self.execute(call_id, request),
            other => self.inner.execute_in(ctx, call_id, other),
        }
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty, clippy::bool_assert_comparison)]
mod tests {
    use super::*;
    use crate::broker::UnavailableExecutor;
    use crate::learning::skills::SkillRoot;
    use serde_json::json;

    fn learning(dir: &std::path::Path) -> Learning {
        skills::clear_cache();
        let ws = dir.join("ws");
        Learning {
            workspace_id: "ws1".into(),
            sandbox_root: ws.clone(),
            memory_dir: dir.join("data/memory/ws1"),
            skills: Skills::new(vec![
                SkillRoot {
                    scope: Scope::Workspace,
                    path: ws.join(".next-agent/skills"),
                },
                SkillRoot {
                    scope: Scope::User,
                    path: dir.join("vault/skills"),
                },
            ]),
            db_path: dir.join("db.sqlite"),
            session_id: Some("s1".into()),
            tools: ToolSet::ALL,
            review: false,
        }
    }

    fn learn(request: Result<ToolRequest, String>) -> LearnRequest {
        match request.expect("request") {
            ToolRequest::Learn(learn) => learn,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn requests_carry_the_policy_target() {
        let dir = tempfile::tempdir().expect("dir");
        let base = std::fs::canonicalize(dir.path()).expect("canon");
        let l = learning(base.as_path());
        let memory = learn(
            l.request("memory", &json!({"target": "user", "operations": []}))
                .expect("offered"),
        );
        assert!(memory.mutating);
        assert_eq!(memory.target, l.memory_dir.join("USER.md"));
        let create = learn(l.request("skill_manage", &json!({"operations": [{"name": "a", "action": "create", "scope": "user", "content": ""}]})).expect("offered"));
        assert_eq!(create.target, base.as_path().join("vault/skills"));
        let list = learn(l.request("skills_list", &json!({})).expect("offered"));
        assert!(!list.mutating);
        assert_eq!(list.target, base.as_path().join("ws/.next-agent"));
        let both = l.request(
            "skill_manage",
            &json!({"operations": [
                {"name": "a", "action": "create", "scope": "user", "content": ""},
                {"name": "b", "action": "create", "content": ""}
            ]}),
        );
        assert!(both
            .expect("offered")
            .expect_err("split")
            .contains("one folder only"));
        assert!(l.request("read_file", &json!({})).is_none());
        let only_search = Learning {
            tools: ToolSet {
                memory: false,
                skills: false,
                session_search: true,
            },
            ..l
        };
        assert!(only_search.request("memory", &json!({})).is_none());
        // Policy sees writers as writes and readers as reads.
        let req = ToolRequest::Learn(memory);
        assert_eq!(
            (req.kind(), req.name(), req.is_mutating()),
            ("write_file", "memory", true)
        );
    }

    #[test]
    fn executes_memory_skills_and_search_and_records_changes() {
        let dir = tempfile::tempdir().expect("dir");
        let base = std::fs::canonicalize(dir.path()).expect("canon");
        let l = learning(base.as_path());
        let mut tools = LearningTools::new(Box::new(UnavailableExecutor), l.clone());
        let changes = tools.changes();
        let call = |tools: &mut LearningTools, name: &str, args: Value| {
            let request = l.request(name, &args).expect("offered").expect("valid");
            tools.execute("c", &request)
        };
        let saved = call(
            &mut tools,
            "memory",
            json!({"target": "memory", "operations": [{"action": "add", "content": "Uses pnpm"}]}),
        );
        assert!(saved.ok, "{}", saved.output);
        let created = call(
            &mut tools,
            "skill_manage",
            json!({"operations": [{"name": "deploy", "action": "create",
            "content": "---\nname: deploy\ndescription: Use when shipping.\n---\nRun make ship.\n"}]}),
        );
        assert!(created.ok, "{}", created.output);
        let viewed = call(&mut tools, "skill_view", json!({"name": "deploy"}));
        assert!(viewed.ok && viewed.output.contains("make ship"));
        let store = Store::open(&l.db_path).expect("store");
        let root = base.join("ws/.next-agent/skills").display().to_string();
        let usage = store.skill_usage(&root).expect("usage");
        assert_eq!(
            (usage[0].created_by.as_deref(), usage[0].view_count),
            (Some("agent"), 1)
        );
        let search = call(&mut tools, "session_search", json!({}));
        assert!(search.ok, "{}", search.output);
        let summaries: Vec<String> = changes
            .lock()
            .expect("lock")
            .iter()
            .map(|c| c.summary.clone())
            .collect();
        assert_eq!(
            summaries,
            ["Memory updated: Entry added.", "Skill 'deploy' created"]
        );
        // Non-learning calls go to the wrapped executor.
        let other = tools.execute("c", &ToolRequest::Read { path: "/x".into() });
        assert!(!other.ok);
    }

    #[test]
    fn repeated_memory_failures_end_with_a_stop_message() {
        let dir = tempfile::tempdir().expect("dir");
        let base = std::fs::canonicalize(dir.path()).expect("canon");
        let l = learning(base.as_path());
        let mut tools = LearningTools::new(Box::new(UnavailableExecutor), l.clone());
        let bad = l.request("memory", &json!({"target": "memory", "operations": [{"action": "remove", "old_text": "nothing"}]})).expect("o").expect("v");
        for _ in 0..4 {
            assert!(!tools.execute("c", &bad).ok);
        }
        assert!(tools
            .execute("c", &bad)
            .output
            .contains("Stop retrying memory calls"));
    }

    #[test]
    fn pinned_skills_survive_skill_manage_delete() {
        let dir = tempfile::tempdir().expect("dir");
        let base = std::fs::canonicalize(dir.path()).expect("canon");
        let l = learning(base.as_path());
        let skill_dir = base.as_path().join("ws/.next-agent/skills/keep");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: keep\ndescription: d\n---\nbody\n",
        )
        .expect("write");
        let store = Store::open(&l.db_path).expect("store");
        store
            .skill_set_pinned(
                &base
                    .as_path()
                    .join("ws/.next-agent/skills")
                    .display()
                    .to_string(),
                "keep",
                true,
                1,
            )
            .expect("pin");
        let mut tools = LearningTools::new(Box::new(UnavailableExecutor), l.clone());
        let delete = l
            .request(
                "skill_manage",
                &json!({"operations": [{"name": "keep", "action": "delete"}]}),
            )
            .expect("o")
            .expect("v");
        let out = tools.execute("c", &delete);
        assert!(!out.ok && out.output.contains("pinned"), "{}", out.output);
        assert!(skill_dir.join("SKILL.md").exists());
    }
}
