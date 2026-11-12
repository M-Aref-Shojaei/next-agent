//! The seam between the permission engine and real tool execution.
//!
//! Every agent tool call passes through [`SqliteBroker::handle`], which:
//! 1. loads the rules in force for the workspace,
//! 2. asks `next_agent_permission_engine` for a decision,
//! 3. resolves `Ask` through an [`Approver`] (fail closed when absent),
//! 4. executes through a [`ToolExecutor`], and
//! 5. appends to the hash-chained audit log when the outcome is noteworthy.
//!
//! Execution is a trait so the OS-facing part (process spawning, file I/O,
//! HTTP) can be swapped or stubbed without touching policy. That is what keeps
//! the policy table updatable independently of the tool implementations.

use crate::runner::{BrokerOutcome, ToolBroker};
use crate::store::{AuditEntry, RuleRow, Store, StoreError};
use crate::tools::{ToolRequest, ToolResult};
use next_agent_permission_engine::{
    evaluate, explain, CommandClass, Context, Decision, Effect, Mode, Request, Rule, RuleSet,
    RuleTarget, Scope, Zone,
};
use std::path::PathBuf;

/// What a human (or an auto-approver policy) chose for an `Ask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Run once.
    Allow,
    /// Run once and persist an allow rule for this workspace.
    AlwaysForWorkspace,
    /// Do not run.
    Deny,
}

/// Resolves `Decision::Ask`. In the app this becomes the approval card.
pub trait Approver {
    fn approve(&mut self, call_id: &str, request: &ToolRequest, reason: &str) -> Approval;
}

/// Performs the side effect once policy has allowed it.
///
/// Deliberately has no policy logic: it must not re-check permissions, or the
/// two layers can disagree.
pub trait ToolExecutor {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult;
}

/// Refuses every call. The safe default until tool implementations land.
#[derive(Debug, Default)]
pub struct UnavailableExecutor;

impl ToolExecutor for UnavailableExecutor {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult {
        ToolResult::error(
            call_id,
            format!("tool `{}` is not implemented yet", request.kind()),
        )
    }
}

/// Broker backed by the metadata database for rules and auditing.
pub struct SqliteBroker<'a> {
    store: &'a Store,
    mode: Mode,
    context: Context,
    rules: RuleSet,
    task_id: Option<String>,
    executor: Box<dyn ToolExecutor + 'a>,
    approver: Option<Box<dyn Approver + 'a>>,
}

impl<'a> SqliteBroker<'a> {
    #[must_use]
    pub fn new(store: &'a Store, mode: Mode, context: Context) -> Self {
        Self {
            store,
            mode,
            context,
            rules: RuleSet::empty(),
            task_id: None,
            executor: Box::new(UnavailableExecutor),
            approver: None,
        }
    }

    #[must_use]
    pub fn with_executor(mut self, executor: impl ToolExecutor + 'a) -> Self {
        self.executor = Box::new(executor);
        self
    }

    #[must_use]
    pub fn with_approver(mut self, approver: impl Approver + 'a) -> Self {
        self.approver = Some(Box::new(approver));
        self
    }

    #[must_use]
    pub fn for_task(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Replace the in-memory rules, e.g. after the user edits preferences.
    pub fn load_rules(&mut self) -> Result<(), StoreError> {
        let rows = self.store.rules_for(&self.context.workspace_id)?;
        self.rules = RuleSet::new(rows.iter().filter_map(row_to_rule).collect());
        Ok(())
    }

    fn audit(&self, request: &ToolRequest, decision: &'static str, reason: &str) {
        let target = describe(request);
        let entry =
            AuditEntry::new(self.mode.label(), request.kind(), &target, decision).reason(reason);
        let entry = match &self.task_id {
            Some(task) => entry.task(task),
            None => entry,
        };
        // A failed audit write must not silently upgrade a denial into a run,
        // but it also must not block an allowed action. Callers see the miss
        // through `last_audit_hash` in the UI's verification view.
        let _ = self.store.append_audit(&entry);
    }
}

impl ToolBroker for SqliteBroker<'_> {
    fn handle(&mut self, call_id: &str, request: &ToolRequest) -> BrokerOutcome {
        let policy_request = to_policy_request(request);
        let evaluation = explain(&policy_request, self.mode, &self.rules, &self.context);

        let reason = match evaluation.decision.clone() {
            Decision::Allow { reason } => reason,
            Decision::Deny { reason } => {
                self.audit(request, "deny", &reason);
                return BrokerOutcome {
                    decision: "deny",
                    reason: reason.clone(),
                    result: ToolResult::error(call_id, reason),
                };
            }
            Decision::Ask { reason } => {
                // Take the choice first: `approve` borrows the approver, and
                // the arms below need `&mut self` to persist or audit.
                let choice = self
                    .approver
                    .as_mut()
                    .map(|approver| approver.approve(call_id, request, &reason));
                match choice {
                    Some(Approval::Allow) => reason,
                    Some(Approval::AlwaysForWorkspace) => {
                        self.persist_allow_rule(request);
                        reason
                    }
                    Some(Approval::Deny) => {
                        self.audit(request, "deny", "user declined the approval");
                        return BrokerOutcome {
                            decision: "deny",
                            reason: "user declined the approval".into(),
                            result: ToolResult::error(call_id, "denied by user"),
                        };
                    }
                    // Fail closed: no approval channel means no side effect.
                    None => {
                        self.audit(request, "ask", &reason);
                        return BrokerOutcome {
                            decision: "ask",
                            reason: reason.clone(),
                            result: ToolResult::error(
                                call_id,
                                format!("approval required but no approver is attached: {reason}"),
                            ),
                        };
                    }
                }
            }
        };

        if noteworthy(self.mode, &evaluation) {
            self.audit(request, "allow", &reason);
        }

        BrokerOutcome {
            decision: "allow",
            reason,
            result: self.executor.execute(call_id, request),
        }
    }
}

impl SqliteBroker<'_> {
    fn persist_allow_rule(&mut self, request: &ToolRequest) {
        let target = describe(request);
        let id = format!(
            "allow-{}-{}",
            request.kind(),
            &crate::hash::sha256_hex(target.as_bytes())[..12]
        );
        let row = RuleRow {
            id,
            scope: "workspace".into(),
            workspace_id: Some(self.context.workspace_id.clone()),
            effect: "allow".into(),
            kind: request.kind().into(),
            pattern: target,
        };
        let _ = self.store.insert_rule(&row);
        let _ = self.load_rules();
    }
}

/// Audit-worthy outcomes: everything in bypass, anything outside the sandbox,
/// and any command that needed a person.
fn noteworthy(mode: Mode, evaluation: &next_agent_permission_engine::Evaluation) -> bool {
    mode.sandbox_unlocked()
        || evaluation.zone.is_some_and(|zone| zone != Zone::Inside)
        || evaluation
            .class
            .is_some_and(|class| class != CommandClass::SafeDev)
}

fn describe(request: &ToolRequest) -> String {
    match request {
        ToolRequest::Read { path }
        | ToolRequest::ListDir { path }
        | ToolRequest::Write { path, .. } => path.display().to_string(),
        ToolRequest::Search { pattern, path } => format!("{} in {}", pattern, path.display()),
        ToolRequest::Exec { command, .. } => command.clone(),
        ToolRequest::Network { url } => url.clone(),
        ToolRequest::AskUser { question } => question.clone(),
    }
}

/// Map the tool layer's request onto the policy crate's dependency-free type.
#[must_use]
pub fn to_policy_request(request: &ToolRequest) -> Request {
    match request {
        ToolRequest::Read { path } => Request::Read { path: path.clone() },
        ToolRequest::ListDir { path } => Request::ListDir { path: path.clone() },
        ToolRequest::Search { path, .. } => Request::Search { path: path.clone() },
        ToolRequest::Write { path, .. } => Request::Write { path: path.clone() },
        ToolRequest::Exec { command, cwd } => Request::Exec {
            command: command.clone(),
            cwd: cwd.clone(),
        },
        ToolRequest::Network { url } => Request::Network { url: url.clone() },
        ToolRequest::AskUser { question } => Request::AskUser {
            question: question.clone(),
        },
    }
}

/// Map a stored row back to a rule, skipping rows from a newer schema.
#[must_use]
pub fn row_to_rule(row: &RuleRow) -> Option<Rule> {
    let effect = match row.effect.as_str() {
        "allow" => Effect::Allow,
        "deny" => Effect::Deny,
        _ => return None,
    };
    let target = if row.kind == "command" {
        RuleTarget::CommandPrefix {
            prefix: row.pattern.split_whitespace().map(str::to_string).collect(),
        }
    } else if row.kind == "path" {
        RuleTarget::Path {
            glob: row.pattern.clone(),
        }
    } else {
        RuleTarget::Tool {
            kind: row.kind.clone(),
        }
    };
    let scope = match (&row.scope[..], row.workspace_id.clone()) {
        ("workspace", Some(id)) => Scope::Workspace { id },
        _ => Scope::Global,
    };
    Some(Rule {
        id: row.id.clone(),
        effect,
        target,
        scope,
    })
}

/// Convenience for callers building a [`Context`] from app settings.
#[must_use]
pub fn context_for(root: PathBuf, workspace_id: String, home: Option<PathBuf>) -> Context {
    Context {
        sandbox: next_agent_permission_engine::Sandbox::new(root, home),
        workspace_id,
        case_insensitive: cfg!(any(target_os = "macos", target_os = "windows")),
        allowlisted_hosts: vec!["docs.rs".into(), "crates.io".into()],
    }
}

/// Sanity check used by the conformance test: the crate's own answer for a
/// read inside the sandbox must be `allow` in `auto`.
#[must_use]
pub fn self_check() -> bool {
    let context = context_for("/work/project".into(), "ws_1".into(), None);
    matches!(
        evaluate(
            &Request::Read {
                path: PathBuf::from("/work/project/a.rs")
            },
            Mode::Auto,
            &RuleSet::empty(),
            &context
        ),
        Decision::Allow { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubExecutor;
    impl ToolExecutor for StubExecutor {
        fn execute(&mut self, call_id: &str, _request: &ToolRequest) -> ToolResult {
            ToolResult::ok(call_id, "ran")
        }
    }

    struct Scripted(Approval);
    impl Approver for Scripted {
        fn approve(&mut self, _call_id: &str, _request: &ToolRequest, _reason: &str) -> Approval {
            self.0
        }
    }

    fn broker(store: &Store, mode: Mode) -> SqliteBroker<'_> {
        SqliteBroker::new(
            store,
            mode,
            context_for("/work/project".into(), "ws_1".into(), None),
        )
        .with_executor(StubExecutor)
    }

    fn read() -> ToolRequest {
        ToolRequest::Read {
            path: "/work/project/src/lib.rs".into(),
        }
    }

    #[test]
    fn allows_inside_read_without_asking() {
        let store = Store::open_in_memory().expect("store");
        let mut broker = broker(&store, Mode::Auto);
        let outcome = broker.handle("c1", &read());
        assert_eq!(outcome.decision, "allow");
        assert!(outcome.result.ok);
    }

    #[test]
    fn catastrophic_command_is_denied_and_audited() {
        let store = Store::open_in_memory().expect("store");
        let mut broker = broker(&store, Mode::BypassPermissions);
        let outcome = broker.handle(
            "c1",
            &ToolRequest::Exec {
                command: "rm -rf /".into(),
                cwd: "/work/project".into(),
            },
        );
        assert_eq!(outcome.decision, "deny");
        assert!(!outcome.result.ok);
        assert_eq!(store.verify_audit().expect("verify").checked, 1);
    }

    #[test]
    fn ask_without_approver_fails_closed() {
        let store = Store::open_in_memory().expect("store");
        let mut broker = broker(&store, Mode::Manual);
        let outcome = broker.handle(
            "c1",
            &ToolRequest::Write {
                path: "/work/project/a.rs".into(),
                contents: "x".into(),
            },
        );
        assert_eq!(outcome.decision, "ask");
        assert!(!outcome.result.ok);
    }

    #[test]
    fn always_writes_a_workspace_rule_that_is_reused() {
        let dir = std::env::temp_dir().join(format!("na-broker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let workspace = crate::Workspace::open(&dir).expect("workspace");

        let store = Store::open_in_memory().expect("store");
        store.upsert_workspace(&workspace).expect("register");

        let context = context_for(
            workspace.root().to_path_buf(),
            workspace.id().to_string(),
            None,
        );
        let mut broker = SqliteBroker::new(&store, Mode::Manual, context)
            .with_executor(StubExecutor)
            .with_approver(Scripted(Approval::AlwaysForWorkspace));

        let write = ToolRequest::Write {
            path: workspace.root().join("a.rs"),
            contents: "x".into(),
        };
        assert_eq!(broker.handle("c1", &write).decision, "allow");
        assert_eq!(broker.rules.len(), 1);
        assert_eq!(store.rules_for(workspace.id()).expect("rules").len(), 1);
        assert_eq!(broker.handle("c2", &write).decision, "allow");
    }

    #[test]
    fn stored_rules_are_loaded_into_the_policy_set() {
        let store = Store::open_in_memory().expect("store");
        store
            .insert_rule(&RuleRow {
                id: "deny-tests".into(),
                scope: "global".into(),
                workspace_id: None,
                effect: "deny".into(),
                kind: "run_command".into(),
                pattern: "pnpm test".into(),
            })
            .expect("rule");

        let mut broker = broker(&store, Mode::Auto);
        broker.load_rules().expect("load");
        let outcome = broker.handle(
            "c1",
            &ToolRequest::Exec {
                command: "pnpm test".into(),
                cwd: "/work/project".into(),
            },
        );
        assert_eq!(outcome.decision, "deny");
    }

    #[test]
    fn rows_from_unknown_schemas_are_ignored() {
        let row = RuleRow {
            id: "x".into(),
            scope: "global".into(),
            workspace_id: None,
            effect: "maybe".into(),
            kind: "run_command".into(),
            pattern: String::new(),
        };
        assert!(row_to_rule(&row).is_none());
    }

    #[test]
    fn self_check_passes() {
        assert!(self_check());
    }
}
