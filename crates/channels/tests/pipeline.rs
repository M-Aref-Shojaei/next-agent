//! Channel tasks through the real pipeline (`spawn_task` → broker →
//! permission engine → local tools): the channel profile only tightens,
//! an unanswered approval becomes deny, plan mode stays read-only, and the
//! SQLite-backed state works.

use next_agent_channels::config::guard_mode;
use next_agent_channels::state::{AllowEntry, ChannelState, StoreState};
use next_agent_channels::{channel_profile, ChannelExecutor, ChannelMode};
use next_agent_core::agent::{AgentStep, MockExecutor, TaskSpec};
use next_agent_core::broker::{context_for, Approval};
use next_agent_core::control::{spawn_task_with, TaskLaunch, TaskOptions};
use next_agent_core::store::RuleRow;
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
    run_with(fx, seed, mode, tools, script, timeout, true, None)
}

/// [`run`] with the origin flag and an optional desktop answer to every
/// approval card.
#[allow(clippy::too_many_arguments)]
fn run_with(
    fx: &Fixture,
    seed: &str,
    mode: ChannelMode,
    tools: &[&str],
    script: Vec<AgentStep>,
    timeout: Duration,
    untrusted_origin: bool,
    answer: Option<Approval>,
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
            untrusted_origin,
            ..TaskOptions::default()
        },
    );
    let mut events = Vec::new();
    while let Ok(event) = handle.events().recv_timeout(Duration::from_secs(10)) {
        if let (EventKind::ApprovalRequested { approval_id, .. }, Some(answer)) =
            (&event.kind, answer)
        {
            handle.respond(approval_id, answer).expect("respond");
        }
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

fn save_allow_rule(fx: &Fixture, kind: &str) {
    Store::open(&fx.db)
        .expect("store")
        .insert_rule(&RuleRow {
            id: format!("rule_{kind}"),
            scope: "workspace".into(),
            workspace_id: Some(fx.workspace.id().to_string()),
            effect: "allow".into(),
            kind: kind.into(),
            pattern: String::new(),
        })
        .expect("rule");
}

fn requested_options(events: &[AgentEvent]) -> Vec<Vec<String>> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::ApprovalRequested { options, .. } => Some(options.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn saved_allow_rules_do_not_auto_approve_channel_tasks() {
    let fx = fixture("rules");
    save_allow_rule(&fx, "write_file");
    // A desktop task: the saved rule upgrades the ask to allow.
    let (_, desktop) = run_with(
        &fx,
        "r1",
        ChannelMode::Manual,
        &["write_file"],
        vec![write(&fx, "desk.txt")],
        Duration::from_millis(150),
        false,
        None,
    );
    assert_eq!(decisions(&desktop)[0].0, "allow");
    // A channel task: the rule is ignored, so it asks; nobody answers: deny.
    let (_, channel) = run_with(
        &fx,
        "r2",
        ChannelMode::Manual,
        &["write_file"],
        vec![write(&fx, "chan.txt")],
        Duration::from_millis(150),
        true,
        None,
    );
    assert_eq!(decisions(&channel)[0].0, "ask");
    assert_eq!(
        requested_options(&channel).len(),
        1,
        "an approval card was shown"
    );
    assert!(!fx.workspace.root().join("chan.txt").exists());
}

#[test]
fn deny_rules_still_apply_to_channel_tasks() {
    let fx = fixture("deny-rule");
    Store::open(&fx.db)
        .expect("store")
        .insert_rule(&RuleRow {
            id: "rule_deny_read".into(),
            scope: "workspace".into(),
            workspace_id: Some(fx.workspace.id().to_string()),
            effect: "deny".into(),
            kind: "read_file".into(),
            pattern: String::new(),
        })
        .expect("rule");
    let script = vec![AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Read {
            path: fx.workspace.root().join("README.md"),
        },
    }];
    let (_, events) = run(
        &fx,
        "dr",
        ChannelMode::Manual,
        &["read_file"],
        script,
        Duration::from_secs(5),
    );
    assert_eq!(decisions(&events)[0].0, "deny");
}

#[test]
fn always_on_a_channel_task_is_offered_never_and_saves_no_rule() {
    let fx = fixture("always");
    let (_, events) = run_with(
        &fx,
        "a1",
        ChannelMode::Manual,
        &["write_file"],
        vec![write(&fx, "once.txt")],
        Duration::from_secs(5),
        true,
        Some(Approval::AlwaysForWorkspace),
    );
    let options = requested_options(&events);
    assert_eq!(
        options,
        [vec!["allow".to_string(), "deny".to_string()]],
        "no always option for channel tasks"
    );
    // An "always" answer (e.g. from an older overlay) runs once and saves nothing.
    assert!(fx.workspace.root().join("once.txt").exists());
    let rules = Store::open(&fx.db)
        .expect("store")
        .rules_for(fx.workspace.id())
        .expect("rules");
    assert_eq!(rules.len(), 0, "no rule saved from a channel task");
}

#[test]
fn secret_files_need_approval_for_channel_tasks() {
    let fx = fixture("secret");
    fs::write(fx.workspace.root().join(".env"), "API_KEY=hunter2\n").expect("env");
    let read = |name: &str| AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Read {
            path: fx.workspace.root().join(name),
        },
    };
    let (_, events) = run(
        &fx,
        "s1",
        ChannelMode::Manual,
        &["read_file"],
        vec![read(".env")],
        Duration::from_millis(150),
    );
    assert_eq!(decisions(&events)[0].0, "ask");
    let leaked = events.iter().any(|e| matches!(&e.kind, EventKind::ToolFinished { summary, .. } if summary.contains("hunter2")));
    assert!(!leaked, "secret contents never reach the task unapproved");
    // Ordinary files still read without a prompt.
    let (_, events) = run(
        &fx,
        "s2",
        ChannelMode::Manual,
        &["read_file"],
        vec![read("README.md")],
        Duration::from_secs(5),
    );
    assert_eq!(decisions(&events)[0].0, "allow");
}

#[cfg(unix)]
#[test]
fn a_link_to_a_secret_file_needs_approval_too() {
    let fx = fixture("secret-link");
    fs::write(fx.workspace.root().join(".env"), "API_KEY=hunter2\n").expect("env");
    std::os::unix::fs::symlink(
        fx.workspace.root().join(".env"),
        fx.workspace.root().join("notes.txt"),
    )
    .expect("link");
    let script = vec![AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Read {
            path: fx.workspace.root().join("notes.txt"),
        },
    }];
    let (_, events) = run(
        &fx,
        "s3",
        ChannelMode::Manual,
        &["read_file"],
        script,
        Duration::from_millis(150),
    );
    assert_eq!(decisions(&events)[0].0, "ask");
}

#[test]
fn enabling_write_file_does_not_enable_edit_or_apply_patch() {
    let fx = fixture("kinds");
    let edit = AgentStep::Call {
        call_id: "c1".into(),
        request: ToolRequest::Edit {
            path: fx.workspace.root().join("README.md"),
            old_string: "hello".into(),
            new_string: "bye".into(),
            replace_all: false,
        },
    };
    let (_, events) = run(
        &fx,
        "k1",
        ChannelMode::Manual,
        &["write_file"],
        vec![edit],
        Duration::from_secs(5),
    );
    let decided = decisions(&events);
    assert_eq!(decided[0].0, "deny");
    assert!(decided[0].1.contains("channel"), "{decided:?}");
    assert!(
        requested_options(&events).is_empty(),
        "denied without a prompt"
    );
}
