//! End-to-end agent execution through `spawn_task`: scripted executor →
//! runner → `SqliteBroker` (permission engine) → `LocalTools`, with every
//! event persisted before it is forwarded.

use next_agent_core::agent::{AgentStep, MockExecutor, TaskSpec};
use next_agent_core::broker::{context_for, Approval};
use next_agent_core::control::{spawn_task, TaskHandle, TaskLaunch};
use next_agent_core::task::{TaskId, TaskStatus};
use next_agent_core::{AgentEvent, LocalTools, Store, ToolRequest, Workspace};
use next_agent_permission_engine::Mode;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

struct Fixture {
    db: PathBuf,
    workspace: Workspace,
}

fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("na-exec-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("project/src")).expect("dirs");
    fs::write(
        dir.join("project/src/lib.rs"),
        "pub fn answer() -> u32 { 42 }\n",
    )
    .expect("seed");
    let workspace = Workspace::open(dir.join("project")).expect("workspace");
    let db = dir.join("meta.db");
    let store = Store::open(&db).expect("store");
    store.upsert_workspace(&workspace).expect("register");
    Fixture { db, workspace }
}

impl Fixture {
    fn root(&self) -> &Path {
        self.workspace.root()
    }

    fn launch(&self, seed: &str, mode: Mode, script: Vec<AgentStep>) -> TaskHandle {
        let id = TaskId::from_seed(seed);
        let label = match mode {
            Mode::Plan => "plan",
            Mode::Manual => "manual",
            Mode::Auto => "auto",
            Mode::BypassPermissions => "bypassPermissions",
        };
        Store::open(&self.db)
            .expect("store")
            .create_task(id.as_str(), self.workspace.id(), seed, "scripted", label)
            .expect("task row");
        spawn_task(TaskLaunch {
            db_path: self.db.clone(),
            spec: TaskSpec {
                id,
                prompt: "scripted".into(),
                mode: label,
                model: "mock-1".into(),
            },
            mode,
            context: context_for(
                self.root().to_path_buf(),
                self.workspace.id().to_string(),
                None,
            ),
            executor: Box::new(MockExecutor::new(script)),
            tools: Box::new(LocalTools::new()),
        })
    }
}

fn call(id: &str, request: ToolRequest) -> AgentStep {
    AgentStep::Call {
        call_id: id.into(),
        request,
    }
}

/// Drain events until the terminal one, answering approvals with `answer`.
fn drive(
    handle: &TaskHandle,
    mut answer: impl FnMut(&AgentEvent) -> Option<Approval>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match handle.events().recv_timeout(remaining) {
            Ok(event) => {
                if event.kind.kind() == "approval_requested" {
                    if let Some(choice) = answer(&event) {
                        let id = event.data()["approval_id"]
                            .as_str()
                            .expect("approval id")
                            .to_string();
                        handle.respond(&id, choice).expect("respond");
                    }
                }
                let terminal = event.kind.is_terminal();
                events.push(event);
                if terminal {
                    return events;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return events,
            Err(RecvTimeoutError::Timeout) => panic!("task did not finish: {events:#?}"),
        }
    }
}

fn kinds(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(|event| event.kind.kind()).collect()
}

fn assert_persisted(db: &Path, events: &[AgentEvent]) {
    let task = events[0].task_id.as_str();
    let stored = Store::open(db)
        .expect("store")
        .task_events(task)
        .expect("events");
    assert_eq!(stored.len(), events.len(), "every event is persisted");
    for ((seq, kind, data), event) in stored.iter().zip(events) {
        assert_eq!(*seq, event.seq);
        assert_eq!(kind, event.kind.kind());
        let data: serde_json::Value = serde_json::from_str(data).expect("json");
        assert_eq!(data, event.data());
    }
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    let expected: Vec<u64> = (0..events.len() as u64).collect();
    assert_eq!(seqs, expected, "gap-free, ordered sequence");
}

#[test]
fn mock_script_runs_through_broker_in_order_and_is_persisted() {
    let fx = fixture("order");
    let handle = fx.launch(
        "order",
        Mode::Auto,
        vec![
            call(
                "c1",
                ToolRequest::Read {
                    path: fx.root().join("src/lib.rs"),
                },
            ),
            call(
                "c2",
                ToolRequest::Search {
                    pattern: "answer".into(),
                    path: fx.root().to_path_buf(),
                },
            ),
        ],
    );
    let events = drive(&handle, |_| None);
    assert_eq!(handle.join(), Ok(TaskStatus::Completed));
    assert_eq!(
        kinds(&events),
        [
            "task_started",
            "message",
            "tool_requested",
            "permission_decided",
            "tool_started",
            "tool_finished",
            "tool_requested",
            "permission_decided",
            "tool_started",
            "tool_finished",
            "task_completed",
        ]
    );
    assert_eq!(
        events[5].data()["summary"],
        "pub fn answer() -> u32 { 42 }\n"
    );
    assert_eq!(
        events[9].data()["summary"],
        "src/lib.rs:1:pub fn answer() -> u32 { 42 }"
    );
    assert_persisted(&fx.db, &events);
    let store = Store::open(&fx.db).expect("store");
    assert_eq!(
        store
            .task_status(events[0].task_id.as_str())
            .expect("status"),
        Some(TaskStatus::Completed)
    );
}

#[test]
fn approval_allow_resumes_the_tool() {
    let fx = fixture("allow");
    let target = fx.root().join("notes.txt");
    let handle = fx.launch(
        "allow",
        Mode::Manual,
        vec![call(
            "w1",
            ToolRequest::Write {
                path: target.clone(),
                contents: "hi".into(),
            },
        )],
    );
    let events = drive(&handle, |_| Some(Approval::Allow));
    assert_eq!(handle.join(), Ok(TaskStatus::Completed));
    assert_eq!(
        kinds(&events),
        [
            "task_started",
            "message",
            "tool_requested",
            "permission_decided",
            "approval_requested",
            "approval_resolved",
            "tool_started",
            "tool_finished",
            "task_completed",
        ]
    );
    assert_eq!(events[3].data()["decision"], "ask");
    assert_eq!(events[4].data()["call_id"], "w1");
    assert_eq!(events[5].data()["choice"], "allow");
    assert_eq!(events[7].data()["ok"], true);
    assert_eq!(fs::read_to_string(target).expect("written"), "hi");
    assert_persisted(&fx.db, &events);
}

#[test]
fn approval_deny_skips_the_tool() {
    let fx = fixture("deny");
    let target = fx.root().join("notes.txt");
    let handle = fx.launch(
        "deny",
        Mode::Manual,
        vec![call(
            "w1",
            ToolRequest::Write {
                path: target.clone(),
                contents: "hi".into(),
            },
        )],
    );
    let events = drive(&handle, |_| Some(Approval::Deny));
    assert_eq!(handle.join(), Ok(TaskStatus::Completed));
    assert!(!kinds(&events).contains(&"tool_started"));
    let finished = events
        .iter()
        .find(|e| e.kind.kind() == "tool_finished")
        .expect("finished");
    assert_eq!(finished.data()["ok"], false);
    assert!(!target.exists());
    assert_persisted(&fx.db, &events);
}

#[test]
fn always_for_workspace_writes_a_scoped_rule() {
    let fx = fixture("always");
    let write = |name: &str| ToolRequest::Write {
        path: fx.root().join(name),
        contents: "x".into(),
    };
    let handle = fx.launch(
        "always",
        Mode::Manual,
        vec![call("w1", write("a.txt")), call("w2", write("b.txt"))],
    );
    let mut asked = 0;
    let events = drive(&handle, |_| {
        asked += 1;
        Some(Approval::AlwaysForWorkspace)
    });
    assert_eq!(handle.join(), Ok(TaskStatus::Completed));
    assert_eq!(asked, 1, "second write is covered by the new rule");
    let rules = Store::open(&fx.db)
        .expect("store")
        .rules_for(fx.workspace.id())
        .expect("rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].scope, "workspace");
    assert_eq!(rules[0].effect, "allow");
    assert!(fx.root().join("b.txt").exists());
    assert_persisted(&fx.db, &events);
}

#[cfg(unix)]
#[test]
fn cancel_mid_tool_kills_the_command() {
    let fx = fixture("cancel");
    let pid_file = fx.root().join("pid");
    let handle = fx.launch(
        "cancel",
        Mode::BypassPermissions,
        vec![
            call(
                "x1",
                ToolRequest::Exec {
                    command: "echo started; echo $$ > pid; sleep 30".into(),
                    cwd: fx.root().to_path_buf(),
                },
            ),
            call(
                "x2",
                ToolRequest::Read {
                    path: fx.root().join("src/lib.rs"),
                },
            ),
        ],
    );
    let started = Instant::now();
    let mut events = Vec::new();
    loop {
        let event = handle
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event");
        let is_output = event.kind.kind() == "tool_output";
        events.push(event);
        if is_output {
            break;
        }
    }
    handle.cancel();
    events.extend(drive(&handle, |_| None));
    assert_eq!(handle.join(), Ok(TaskStatus::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel is prompt"
    );

    let names = kinds(&events);
    assert_eq!(names.last(), Some(&"task_cancelled"));
    assert_eq!(
        names.iter().filter(|k| **k == "tool_requested").count(),
        1,
        "no further calls after cancel"
    );
    let finished = events
        .iter()
        .find(|e| e.kind.kind() == "tool_finished")
        .expect("finished");
    assert_eq!(finished.data()["ok"], false);
    assert!(finished.data()["summary"]
        .as_str()
        .is_some_and(|s| s.starts_with("cancelled")));

    // The shell was killed, not left running in the background.
    let pid = fs::read_to_string(&pid_file)
        .expect("pid")
        .trim()
        .to_string();
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("kill -0")
        .success();
    assert!(!alive, "process {pid} survived cancel");
    assert_persisted(&fx.db, &events);
    assert_eq!(
        Store::open(&fx.db)
            .expect("store")
            .task_status(events[0].task_id.as_str())
            .expect("status"),
        Some(TaskStatus::Cancelled)
    );
}

#[test]
fn cancel_while_waiting_for_approval() {
    let fx = fixture("cancel-approval");
    let handle = fx.launch(
        "cancel-approval",
        Mode::Manual,
        vec![call(
            "w1",
            ToolRequest::Write {
                path: fx.root().join("never.txt"),
                contents: "x".into(),
            },
        )],
    );
    let control = handle.control();
    let events = drive(&handle, move |_| {
        control.cancel();
        None
    });
    assert_eq!(handle.join(), Ok(TaskStatus::Cancelled));
    let names = kinds(&events);
    assert!(names.contains(&"approval_resolved"));
    assert_eq!(names.last(), Some(&"task_cancelled"));
    assert!(!fx.root().join("never.txt").exists());
    assert_persisted(&fx.db, &events);
}

#[test]
fn plan_mode_denies_writes_without_asking() {
    let fx = fixture("plan");
    let handle = fx.launch(
        "plan",
        Mode::Plan,
        vec![call(
            "w1",
            ToolRequest::ApplyPatch {
                path: fx.root().join("src/lib.rs"),
                patch:
                    "@@ -1 +1 @@\n-pub fn answer() -> u32 { 42 }\n+pub fn answer() -> u32 { 0 }\n"
                        .into(),
            },
        )],
    );
    let events = drive(&handle, |_| panic!("plan mode must not ask"));
    assert_eq!(handle.join(), Ok(TaskStatus::Completed));
    let decided = events
        .iter()
        .find(|e| e.kind.kind() == "permission_decided")
        .expect("decided");
    assert_eq!(decided.data()["decision"], "deny");
    let requested = events
        .iter()
        .find(|e| e.kind.kind() == "tool_requested")
        .expect("requested");
    assert_eq!(requested.data()["tool"], "apply_patch");
    assert_eq!(
        fs::read_to_string(fx.root().join("src/lib.rs")).expect("read"),
        "pub fn answer() -> u32 { 42 }\n"
    );
}

#[test]
fn missing_task_row_fails_persistence_and_the_task() {
    let fx = fixture("orphan");
    let id = TaskId::from_seed("orphan");
    let handle = spawn_task(TaskLaunch {
        db_path: fx.db.clone(),
        spec: TaskSpec {
            id,
            prompt: "x".into(),
            mode: "auto",
            model: "m".into(),
        },
        mode: Mode::Auto,
        context: context_for(fx.root().to_path_buf(), fx.workspace.id().to_string(), None),
        executor: Box::new(MockExecutor::new(vec![])),
        tools: Box::new(LocalTools::new()),
    });
    // Unpersisted events are never forwarded.
    assert!(handle
        .events()
        .recv_timeout(Duration::from_secs(10))
        .is_err());
    assert_eq!(handle.join(), Ok(TaskStatus::Failed));
}
