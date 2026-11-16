//! A scripted task whose prompt and tool arguments carry a canary must leave
//! no trace of it in the events the core writes through `tracing`
//! (ADR-0013: log app behaviour, never what people work on).
//!
//! The capture is a minimal `tracing` subscriber that records every event
//! name, field and span field, as the diagnostics layer would see them,
//! before the allowlist is applied: a leak here is a leak the allowlist
//! would have to catch.

use next_agent_core::agent::{AgentStep, MockExecutor, TaskSpec};
use next_agent_core::broker::{context_for, Approval, Approver, SqliteBroker};
use next_agent_core::runner::run_task;
use next_agent_core::task::TaskId;
use next_agent_core::{LocalTools, Store, ToolRequest, Workspace};
use next_agent_permission_engine::Mode;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const CANARY: &str = "CANARY-PROMPT-7f3a";

#[derive(Default)]
struct Line(String);

impl Visit for Line {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

/// Every event and span as one text line, all targets and levels.
#[derive(Clone, Default)]
struct Capture {
    lines: Arc<Mutex<Vec<String>>>,
    spans: Arc<Mutex<HashMap<u64, String>>>,
    next: Arc<AtomicU64>,
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut line = Line::default();
        attributes.record(&mut line);
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let text = format!("span {}{}", attributes.metadata().name(), line.0);
        self.spans.lock().expect("spans").insert(id, text.clone());
        self.lines.lock().expect("lines").push(text);
        Id::from_u64(id)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut line = Line::default();
        event.record(&mut line);
        self.lines.lock().expect("lines").push(format!(
            "{} {}{}",
            event.metadata().target(),
            event.metadata().name(),
            line.0
        ));
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// Answers every approval: the first "always for workspace", the rest allow.
struct Approves(bool);

impl Approver for Approves {
    fn approve(&mut self, _: &str, _: &ToolRequest, _: &str) -> Approval {
        if std::mem::replace(&mut self.0, false) {
            Approval::AlwaysForWorkspace
        } else {
            Approval::Allow
        }
    }
}

#[test]
fn a_scripted_task_logs_no_prompt_or_tool_arguments() {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::create_dir_all(dir.path().join("project")).expect("project");
    let workspace = Workspace::open(dir.path().join("project")).expect("workspace");
    let store = Store::open(dir.path().join("meta.db")).expect("store");
    store.upsert_workspace(&workspace).expect("register");
    let root = workspace.root().to_path_buf();

    let call = |id: &str, request| AgentStep::Call {
        call_id: id.into(),
        request,
    };
    let script = vec![
        call(
            "c1",
            ToolRequest::Exec {
                command: format!("echo {CANARY}"),
                cwd: root.clone(),
            },
        ),
        call(
            "c2",
            ToolRequest::Write {
                path: root.join(format!("{CANARY}.txt")),
                contents: format!("{CANARY} file contents"),
            },
        ),
        call(
            "c3",
            ToolRequest::Network {
                url: format!("https://example.com/{CANARY}?q={CANARY}"),
            },
        ),
        call(
            "c4",
            ToolRequest::Read {
                path: root.join(format!("{CANARY}.txt")),
            },
        ),
        AgentStep::Done {
            summary: format!("{CANARY} done"),
        },
    ];
    let id = TaskId::from_seed("canary");
    store
        .create_task(id.as_str(), workspace.id(), CANARY, "scripted", "manual")
        .expect("task row");
    let spec = TaskSpec {
        id,
        prompt: format!("please work on {CANARY}"),
        mode: "manual",
        model: "mock-1".into(),
    };

    let capture = Capture::default();
    let status = tracing::subscriber::with_default(capture.clone(), || {
        let mut broker = SqliteBroker::new(
            &store,
            Mode::Manual,
            context_for(root.clone(), workspace.id().to_string(), None),
        )
        .with_executor(LocalTools::new())
        .with_approver(Approves(true))
        .for_task(spec.id.as_str());
        let mut executor = MockExecutor::new(script);
        let events = run_task(&mut executor, &mut broker, &spec, &|| false);
        // The task's own event stream does carry the arguments (that is the
        // timeline the user sees); the log must not.
        assert!(events.iter().any(|e| format!("{e:?}").contains(CANARY)));
        events.last().map(|e| e.kind.kind())
    });
    assert_eq!(status, Some("task_completed"));

    let lines = capture.lines.lock().expect("lines").clone();
    let names = |needle: &str| lines.iter().filter(|l| l.contains(needle)).count();
    assert_eq!(names("task_started"), 1, "{lines:#?}");
    assert_eq!(names("task_finished"), 1, "{lines:#?}");
    assert!(names("tool_decision") >= 3, "{lines:#?}");
    assert!(names("rule_added") >= 1, "{lines:#?}");
    for line in &lines {
        assert!(
            !line.contains(CANARY),
            "canary leaked into a log line: {line}"
        );
        assert!(!line.contains("example.com"), "a URL leaked: {line}");
    }
}
