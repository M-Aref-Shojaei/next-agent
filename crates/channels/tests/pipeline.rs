//! Channel tasks through the real pipeline (`spawn_task` → broker →
//! permission engine → local tools): the channel profile only tightens,
//! an unanswered approval becomes deny, plan mode stays read-only, and the
//! SQLite-backed state works.

use next_agent_channels::config::guard_mode;
use next_agent_channels::state::{AllowEntry, ChannelState, StoreState};
use next_agent_channels::{channel_profile, ChannelExecutor, ChannelMode};
use next_agent_core::agent::{AgentStep, MockExecutor, TaskSpec};
use next_agent_core::broker::context_for;
use next_agent_core::control::{spawn_task_with, TaskLaunch, TaskOptions};
use next_agent_core::task::{TaskId, TaskStatus};
use next_agent_core::{AgentEvent, EventKind, LocalTools, Store, ToolRequest, Workspace};
use next_agent_permission_engine::{Mode, ModeError, Origin, Transition};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    db: PathBuf,
    workspace: Workspace,
}

fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("na-channels-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("project")).expect("dirs");
    fs::write(dir.join("project/README.md"), "hello\n").expect("seed");
    let workspace = Workspace::open(dir.join("project")).expect("workspace");
    let db = dir.join("meta.db");
    Store::open(&db)
        .expect("store")
        .upsert_workspace(&workspace)
        .expect("register");
    Fixture { db, workspace }
}

/// Run `script` as a channel task: the channel profile around the executor,
/// the mode through `guard_mode`, approvals timing out after `timeout`.
fn run(
    fx: &Fixture,
    seed: &str,
    mode: ChannelMode,
    tools: &[&str],
    script: Vec<AgentStep>,
    timeout: Duration,
) -> (TaskStatus, Vec<AgentEvent>) {
    let mode = guard_mode(mode.mode()).expect("channel mode");
    let id = TaskId::from_seed(seed);
    Store::open(&fx.db)
        .expect("store")
        .create_task(
            id.as_str(),
            fx.workspace.id(),
            seed,
            "channel",
            ChannelMode::Manual.as_str(),
        )
        .expect("task row");
    let tools: Vec<String> = tools.iter().map(|t| (*t).to_string()).collect();
    let executor =
        ChannelExecutor::new(Box::new(MockExecutor::new(script)), channel_profile(&tools));
    let handle = spawn_task_with(
        TaskLaunch {
            db_path: fx.db.clone(),
            spec: TaskSpec {
                id,
                prompt: "channel".into(),
                mode: "manual",
                model: "mock".into(),
            },
            mode,
            context: context_for(
                fx.workspace.root().to_path_buf(),
                fx.workspace.id().to_string(),
                None,
            ),
            executor: Box::new(executor),
            tools: Box::new(LocalTools::new()),
        },
        TaskOptions {
            approval_timeout: Some(timeout),
            ..TaskOptions::default()
        },
    );
    let mut events = Vec::new();
    while let Ok(event) = handle.events().recv_timeout(Duration::from_secs(10)) {
        events.push(event);
    }
    (handle.join().expect("join"), events)
}

fn decisions(events: &[AgentEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::PermissionDecided {
                decision, reason, ..
            } => Some((decision.clone(), reason.clone())),
            _ => None,
        })
        .collect()
}

fn write(fx: &Fixture, name: &str) -> AgentStep {
    AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Write {
            path: fx.workspace.root().join(name),
            contents: "x".into(),
        },
    }
}

#[test]
fn unanswered_approval_times_out_as_deny() {
    let fx = fixture("timeout");
    let (status, events) = run(
        &fx,
        "t1",
        ChannelMode::Manual,
        &["write_file"],
        vec![write(&fx, "out.txt")],
        Duration::from_millis(150),
    );
    assert_eq!(status, TaskStatus::Completed);
    let resolved = events.iter().find_map(|e| match &e.kind {
        EventKind::ApprovalResolved { choice, by, .. } => Some((choice.clone(), by.clone())),
        _ => None,
    });
    assert_eq!(resolved, Some(("deny".into(), "timeout".into())));
    assert!(
        !fx.workspace.root().join("out.txt").exists(),
        "nothing written without an approval"
    );
}

#[test]
fn tools_outside_the_channel_profile_are_denied_without_asking() {
    let fx = fixture("floor");
    let script = vec![AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Exec {
            command: "ls".into(),
            cwd: fx.workspace.root().to_path_buf(),
        },
    }];
    let (_, events) = run(
        &fx,
        "t2",
        ChannelMode::Manual,
        &["read_file"],
        script,
        Duration::from_secs(5),
    );
    let decided = decisions(&events);
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].0, "deny");
    assert!(decided[0].1.contains("channel"), "{decided:?}");
    assert!(!events
        .iter()
        .any(|e| matches!(e.kind, EventKind::ApprovalRequested { .. })));
}

#[test]
fn reads_inside_the_workspace_run_through_the_engine() {
    let fx = fixture("read");
    let script = vec![AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Read {
            path: fx.workspace.root().join("README.md"),
        },
    }];
    let (_, events) = run(
        &fx,
        "t3",
        ChannelMode::Manual,
        &["read_file"],
        script,
        Duration::from_secs(5),
    );
    assert_eq!(decisions(&events)[0].0, "allow");
}

#[test]
fn plan_mode_denies_writes_even_when_the_profile_allows_them() {
    let fx = fixture("plan");
    let (_, events) = run(
        &fx,
        "t4",
        ChannelMode::Plan,
        &["write_file"],
        vec![write(&fx, "plan.txt")],
        Duration::from_secs(5),
    );
    assert_eq!(decisions(&events)[0].0, "deny");
    assert!(!fx.workspace.root().join("plan.txt").exists());
}

#[test]
fn channel_tasks_can_never_enter_bypass() {
    assert!(guard_mode(Mode::BypassPermissions).is_err());
    assert!(guard_mode(Mode::Auto).is_err());
    assert!(ChannelMode::parse("bypassPermissions").is_err());
    // The permission engine refuses it for the channel origin too.
    assert_eq!(
        Mode::Manual.transition(Transition::Set(Mode::BypassPermissions), Origin::Channel),
        Err(ModeError::BypassNeedsConfirmation)
    );
}

#[test]
fn sqlite_state_round_trips() {
    let fx = fixture("state");
    let state = StoreState::open(&fx.db).expect("state");
    assert!(state.claim("in:ch:1", "").expect("claim"));
    assert!(!state.claim("in:ch:1", "").expect("again"));
    state
        .allow_add(
            "ch",
            &AllowEntry {
                sender_id: "42".into(),
                label: "@a".into(),
                source: "pairing".into(),
                added_at: String::new(),
            },
        )
        .expect("allow");
    assert_eq!(state.allow_list("ch").expect("list")[0].sender_id, "42");
    let request = next_agent_channels::PairingRequest {
        channel_id: "ch".into(),
        sender_id: "7".into(),
        chat_id: "7".into(),
        label: "@b".into(),
        code: "ABCD2345".into(),
        created_at: 5,
        last_seen_at: 6,
    };
    state
        .set_pairings("ch", std::slice::from_ref(&request))
        .expect("pairings");
    assert_eq!(state.pairings("ch").expect("read"), [request]);
    state
        .log(&next_agent_channels::Activity {
            channel_id: "ch".into(),
            direction: "in".into(),
            kind: "message".into(),
            text: "hi".into(),
            ..Default::default()
        })
        .expect("log");
    let activity = state.activity("ch", 5).expect("activity");
    assert_eq!(activity[0].text, "hi");
    assert_ne!(activity[0].ts.len(), 0);
}
