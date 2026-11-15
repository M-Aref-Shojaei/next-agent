//! Memory, skills and the background review end to end: the built-in
//! executor → runner → `SqliteBroker` (permission engine) → `LearningTools`.
#![allow(
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::bool_assert_comparison
)]

use next_agent_core::agent::TaskSpec;
use next_agent_core::broker::{context_for, SqliteBroker};
use next_agent_core::learning::memory::{MemoryOp, MemoryStore, Target};
use next_agent_core::learning::review::{self, ReviewApprover};
use next_agent_core::learning::sessions::{SessionRecorder, Transcript};
use next_agent_core::learning::skills::{self, Scope, SkillRoot, Skills};
use next_agent_core::learning::{Learning, LearningTools, ToolSet};
use next_agent_core::llm::{
    ChatRequest, Completion, CompletionClient, FunctionCall, LlmError, ToolCall,
};
use next_agent_core::runner::run_task;
use next_agent_core::store::SessionRow;
use next_agent_core::task::TaskId;
use next_agent_core::{AgentEvent, BuiltinExecutor, LocalTools, Store, Workspace};
use next_agent_permission_engine::Mode;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct Scripted {
    replies: Mutex<VecDeque<Completion>>,
    seen: Mutex<Vec<ChatRequest>>,
}

impl Scripted {
    fn new(replies: Vec<Completion>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
        })
    }
    fn seen(&self) -> Vec<ChatRequest> {
        self.seen.lock().expect("lock").clone()
    }
}

impl CompletionClient for Scripted {
    fn complete(
        &self,
        request: &ChatRequest,
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        self.seen.lock().expect("lock").push(request.clone());
        Ok(self
            .replies
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or_default())
    }
}

fn calls(calls: &[(&str, &str, Value)]) -> Completion {
    Completion {
        tool_calls: calls
            .iter()
            .map(|(id, name, args)| ToolCall {
                id: (*id).into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: (*name).into(),
                    arguments: args.to_string(),
                },
            })
            .collect(),
        ..Completion::default()
    }
}

fn text(content: &str) -> Completion {
    Completion {
        content: content.into(),
        ..Completion::default()
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    workspace: Workspace,
    learning: Learning,
}

fn fixture() -> Fixture {
    skills::clear_cache();
    let dir = tempfile::tempdir().expect("dir");
    let base = fs::canonicalize(dir.path()).expect("canon");
    let root = base.join("project");
    fs::create_dir_all(root.join("src")).expect("dirs");
    fs::write(root.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").expect("seed");
    let workspace = Workspace::open(&root).expect("workspace");
    let db = base.join("meta.db");
    Store::open(&db)
        .expect("store")
        .upsert_workspace(&workspace)
        .expect("register");
    let learning = Learning {
        workspace_id: workspace.id().to_string(),
        sandbox_root: root.clone(),
        memory_dir: base.join("data/memory").join(workspace.id()),
        skills: Skills::new(vec![
            SkillRoot {
                scope: Scope::Workspace,
                path: root.join(".next-agent/skills"),
            },
            SkillRoot {
                scope: Scope::User,
                path: base.join("vault/skills"),
            },
        ]),
        db_path: db.clone(),
        session_id: None,
        tools: ToolSet::ALL,
        review: false,
    };
    Fixture {
        _dir: dir,
        root,
        db,
        workspace,
        learning,
    }
}

impl Fixture {
    fn spec(&self, seed: &str, mode: &'static str) -> TaskSpec {
        let id = TaskId::from_seed(seed);
        Store::open(&self.db)
            .expect("store")
            .create_task(id.as_str(), self.workspace.id(), seed, "p", mode)
            .expect("task");
        TaskSpec {
            id,
            prompt: "Do the thing.".into(),
            mode,
            model: "m".into(),
        }
    }

    fn run(
        &self,
        executor: &mut BuiltinExecutor,
        mode: Mode,
        spec: &TaskSpec,
        review: bool,
    ) -> Vec<AgentEvent> {
        let store = Store::open(&self.db).expect("store");
        let learning = Learning {
            review,
            ..self.learning.clone()
        };
        let tools = LearningTools::new(Box::new(LocalTools::new()), learning);
        let mut broker = SqliteBroker::new(
            &store,
            mode,
            context_for(self.root.clone(), self.workspace.id().to_string(), None),
        )
        .for_task(spec.id.as_str())
        .with_executor(tools);
        if review {
            broker = broker.with_approver(ReviewApprover);
        }
        broker.load_rules().expect("rules");
        run_task(executor, &mut broker, spec, &|| false)
    }
}

fn tool_names(request: &ChatRequest) -> Vec<String> {
    request
        .tools
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

fn finished(events: &[AgentEvent]) -> Vec<(String, bool, String)> {
    events
        .iter()
        .filter(|e| e.kind.kind() == "tool_finished")
        .map(|e| {
            let data = e.data();
            (
                data["call_id"].as_str().unwrap_or_default().to_string(),
                data["ok"] == true,
                data["summary"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn the_snapshot_is_frozen_in_the_first_user_message_and_the_system_prompt_never_changes() {
    let f = fixture();
    let memory = MemoryStore::new(&f.learning.memory_dir);
    assert_eq!(
        memory.apply(
            Target::User,
            &[MemoryOp::Add {
                content: "Name: Sam".into()
            }],
            false
        )["success"],
        true
    );
    let client = Scripted::new(vec![
        calls(&[(
            "c1",
            "memory",
            json!({"target": "memory", "operations": [{"action": "add", "content": "Tests run with cargo nextest"}]}),
        )]),
        calls(&[("c2", "skills_list", json!({}))]),
        text("Done."),
    ]);
    let mut executor =
        BuiltinExecutor::new(client.clone(), &f.root).with_learning(f.learning.clone());
    let spec = f.spec("frozen", "bypassPermissions");
    let events = f.run(&mut executor, Mode::BypassPermissions, &spec, false);
    assert!(
        finished(&events).iter().all(|(_, ok, _)| *ok),
        "{:?}",
        finished(&events)
    );
    // The write reached the disk.
    assert_eq!(
        memory.entries(Target::Memory).expect("read"),
        ["Tests run with cargo nextest"]
    );
    let seen = client.seen();
    assert_eq!(seen.len(), 3);
    let system = seen[0].messages[0].content.clone().expect("system");
    assert!(system.contains("You have persistent memory"));
    assert!(
        !system.contains("Name: Sam"),
        "stored text never enters the system prompt"
    );
    for request in &seen {
        assert_eq!(
            request.messages[0].content.as_deref(),
            Some(system.as_str())
        );
        let first = request.messages[1].content.as_deref().expect("first user");
        assert!(first.starts_with("<memory-snapshot>"));
        assert!(first.contains("Name: Sam"));
        assert!(
            !first.contains("cargo nextest"),
            "the snapshot stays frozen for the task"
        );
        assert!(first.ends_with("Do the thing."));
    }
    assert!(tool_names(&seen[0]).contains(&"memory".to_string()));
    assert!(tool_names(&seen[0]).contains(&"session_search".to_string()));

    // The next task sees the new entry.
    let next = Scripted::new(vec![text("ok")]);
    let mut executor =
        BuiltinExecutor::new(next.clone(), &f.root).with_learning(f.learning.clone());
    let spec = f.spec("next", "bypassPermissions");
    f.run(&mut executor, Mode::BypassPermissions, &spec, false);
    assert!(next.seen()[0].messages[1]
        .content
        .as_deref()
        .expect("first")
        .contains("cargo nextest"));
}

#[test]
fn memory_and_skill_writes_go_through_the_permission_engine() {
    let f = fixture();
    let client = Scripted::new(vec![
        calls(&[
            (
                "c1",
                "memory",
                json!({"target": "memory", "operations": [{"action": "add", "content": "a fact"}]}),
            ),
            (
                "c2",
                "skill_manage",
                json!({"operations": [{"name": "deploy", "action": "create", "content": "---\nname: deploy\ndescription: Use when shipping.\n---\nRun make ship.\n"}]}),
            ),
        ]),
        text("Done."),
    ]);
    // Manual mode asks; with no approval channel the broker fails closed.
    let mut executor = BuiltinExecutor::new(client, &f.root).with_learning(f.learning.clone());
    let spec = f.spec("manual", "manual");
    let events = f.run(&mut executor, Mode::Manual, &spec, false);
    let results = finished(&events);
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|(_, ok, out)| !ok && out.contains("approval required")),
        "{results:?}"
    );
    assert!(!f.learning.memory_dir.join("MEMORY.md").exists());
    assert!(!f.root.join(".next-agent/skills/deploy").exists());
    let decided: Vec<String> = events
        .iter()
        .filter(|e| e.kind.kind() == "permission_decided")
        .map(|e| {
            e.data()["decision"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(decided, ["ask", "ask"]);
}

#[test]
fn plan_mode_offers_no_learning_writes_and_the_broker_denies_them() {
    let f = fixture();
    let client = Scripted::new(vec![
        calls(&[(
            "c1",
            "memory",
            json!({"target": "memory", "operations": [{"action": "add", "content": "x"}]}),
        )]),
        text("Plan."),
    ]);
    let mut executor =
        BuiltinExecutor::new(client.clone(), &f.root).with_learning(f.learning.clone());
    let spec = f.spec("plan", "plan");
    let events = f.run(&mut executor, Mode::Plan, &spec, false);
    let offered = tool_names(&client.seen()[0]);
    assert!(!offered.contains(&"memory".to_string()));
    assert!(!offered.contains(&"skill_manage".to_string()));
    assert!(offered.contains(&"skill_view".to_string()));
    assert!(offered.contains(&"session_search".to_string()));
    // The model called it anyway: refused without reaching the broker.
    assert!(!events.iter().any(|e| e.kind.kind() == "tool_requested"));
    assert!(!f.learning.memory_dir.join("MEMORY.md").exists());
}

#[test]
fn the_review_fork_is_limited_to_memory_and_skills_and_every_write_is_audited() {
    let f = fixture();
    let skill_dir = f.root.join(".next-agent/skills/deploy");
    fs::create_dir_all(&skill_dir).expect("dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: deploy\ndescription: Use when shipping.\n---\nRun make ship.\n",
    )
    .expect("skill");
    let client = Scripted::new(vec![
        calls(&[
            (
                "c1",
                "write_file",
                json!({"path": "src/x.rs", "contents": "evil"}),
            ),
            ("c2", "run_command", json!({"command": "rm -rf src"})),
            (
                "c3",
                "skill_manage",
                json!({"operations": [{"name": "deploy", "action": "patch", "old_string": "make ship", "new_string": "make release"}]}),
            ),
            (
                "c4",
                "memory",
                json!({"target": "memory", "operations": [{"action": "add", "content": "Releases use make release"}]}),
            ),
            (
                "c5",
                "skill_manage",
                json!({"operations": [{"name": "deploy", "action": "delete", "absorbed_into": ""}]}),
            ),
        ]),
        calls(&[("c6", "skill_view", json!({"name": "deploy"}))]),
        calls(&[(
            "c7",
            "skill_manage",
            json!({"operations": [{"name": "deploy", "action": "patch", "old_string": "make ship", "new_string": "make release"}]}),
        )]),
        text("Saved one memory entry and patched deploy."),
    ]);
    let mut executor = BuiltinExecutor::new(client.clone(), &f.root)
        .with_profile(review::profile())
        .with_step_budget(review::MAX_ITERATIONS)
        .with_learning(Learning {
            review: true,
            tools: ToolSet {
                session_search: false,
                ..ToolSet::ALL
            },
            ..f.learning.clone()
        });
    let spec = f.spec("review", "manual");
    let events = f.run(&mut executor, Mode::Manual, &spec, true);

    let offered = tool_names(&client.seen()[0]);
    let mut sorted = offered.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "memory",
            "read_file",
            "search",
            "skill_manage",
            "skill_view",
            "skills_list"
        ]
    );
    // write_file and run_command never reached the broker.
    let requested: Vec<String> = events
        .iter()
        .filter(|e| e.kind.kind() == "tool_requested")
        .map(|e| e.data()["call_id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(requested, ["c3", "c4", "c5", "c6", "c7"]);
    assert!(!f.root.join("src/x.rs").exists());
    assert!(f.root.join("src/lib.rs").exists());
    let results = finished(&events);
    let result = |id: &str| results.iter().find(|r| r.0 == id).cloned().expect(id);
    assert!(
        !result("c3").1 && result("c3").2.contains("Read-before-write"),
        "{:?}",
        result("c3")
    );
    assert!(result("c4").1, "{:?}", result("c4"));
    assert!(!result("c5").1 && result("c5").2.contains("Never delete"));
    assert!(result("c7").1, "{:?}", result("c7"));
    assert!(fs::read_to_string(skill_dir.join("SKILL.md"))
        .expect("md")
        .contains("make release"));
    // Writes were approved by the review setting and audited as such.
    let store = Store::open(&f.db).expect("store");
    let audit = store
        .audit_recent(Some(spec.id.as_str()), 20)
        .expect("audit");
    let approved: Vec<&str> = audit
        .iter()
        .filter(|r| r.4.contains("background-review"))
        .map(|r| r.1.as_str())
        .collect();
    assert!(approved.len() >= 3, "{audit:?}");
    let resolved: Vec<String> = events
        .iter()
        .filter(|e| e.kind.kind() == "approval_resolved")
        .map(|e| e.data()["by"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        resolved.iter().all(|by| by == "background-review"),
        "{resolved:?}"
    );
}

#[test]
fn transcripts_are_recorded_and_searchable_from_the_next_task() {
    let f = fixture();
    let spec = f.spec("first", "bypassPermissions");
    let session = SessionRow {
        id: spec.id.as_str().into(),
        workspace_id: f.workspace.id().into(),
        task_id: Some(spec.id.as_str().into()),
        source: "task".into(),
        started_at: 1,
        title: Some("first".into()),
        ..SessionRow::default()
    };
    let recorder = SessionRecorder::open(&f.db, &session).expect("recorder");
    let client = Scripted::new(vec![
        calls(&[("c1", "read_file", json!({"path": "src/lib.rs"}))]),
        text("The answer function returns 42."),
    ]);
    let mut executor = BuiltinExecutor::new(client, &f.root).with_transcript(Box::new(recorder));
    f.run(&mut executor, Mode::BypassPermissions, &spec, false);
    let store = Store::open(&f.db).expect("store");
    let messages = store
        .session_messages(spec.id.as_str(), false)
        .expect("messages");
    let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
    assert_eq!(messages[2].tool_name.as_deref(), Some("read_file"));
    assert!(store
        .session_get(spec.id.as_str())
        .expect("get")
        .expect("row")
        .system_prompt_hash
        .is_some());

    let second = f.spec("second", "bypassPermissions");
    let client = Scripted::new(vec![
        calls(&[("c1", "session_search", json!({"query": "answer function"}))]),
        text("Found it."),
    ]);
    let learning = Learning {
        session_id: Some(second.id.as_str().into()),
        ..f.learning.clone()
    };
    let mut executor = BuiltinExecutor::new(client, &f.root).with_learning(learning);
    let events = f.run(&mut executor, Mode::BypassPermissions, &second, false);
    let found = finished(&events);
    assert!(
        found[0].1 && found[0].2.contains(spec.id.as_str()),
        "{found:?}"
    );
}

#[test]
fn recorder_implements_transcript() {
    fn takes(_: &dyn Transcript) {}
    let f = fixture();
    let row = SessionRow {
        id: "x".into(),
        workspace_id: "w".into(),
        source: "task".into(),
        started_at: 1,
        ..SessionRow::default()
    };
    let recorder = SessionRecorder::open(Path::new(&f.db), &row).expect("recorder");
    takes(&recorder);
}

#[test]
fn the_broker_itself_denies_review_calls_outside_the_allowed_set_even_in_bypass() {
    use next_agent_core::agent::AgentExecutor;
    use next_agent_core::events::Emitter;
    use next_agent_core::runner::{Floor, RunContext, ToolBroker};
    use next_agent_core::ToolRequest;

    let f = fixture();
    let client = Scripted::new(vec![text("Nothing to save.")]);
    let mut executor = BuiltinExecutor::new(client, &f.root)
        .with_profile(review::profile())
        .with_learning(Learning {
            review: true,
            ..f.learning.clone()
        });
    let spec = f.spec("floor", "bypassPermissions");
    let _ = executor.start(&spec);
    let write = ToolRequest::Write {
        path: f.root.join("src/evil.rs"),
        contents: "x".into(),
    };
    let exec = ToolRequest::Exec {
        command: "touch pwned".into(),
        cwd: f.root.clone(),
    };
    for request in [&write, &exec] {
        // Even if a call got past the tool list, the profile floor rides with it.
        let floor = executor.floor("c1", request);
        assert!(
            matches!(floor, Some(Floor::Deny(_))),
            "{request:?}: {floor:?}"
        );
        let store = Store::open(&f.db).expect("store");
        let mut broker = SqliteBroker::new(
            &store,
            Mode::BypassPermissions,
            context_for(f.root.clone(), f.workspace.id().to_string(), None),
        )
        .with_executor(LocalTools::new())
        .with_approver(ReviewApprover);
        let events = Emitter::new(spec.id.clone());
        let ctx = RunContext {
            events: &events,
            cancelled: &|| false,
        };
        let outcome = broker.handle_floored(&ctx, "c1", request, floor.as_ref());
        assert_eq!(outcome.decision, "deny", "{request:?}");
    }
    assert!(!f.root.join("src/evil.rs").exists());
    assert!(!f.root.join("pwned").exists());
    // The learning writers stay allowed by the profile.
    let memory = f
        .learning
        .request("memory", &json!({"target": "memory", "operations": []}))
        .expect("offered")
        .expect("valid");
    assert_eq!(executor.floor("c2", &memory), None);
}
