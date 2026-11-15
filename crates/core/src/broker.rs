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

use crate::events::EventKind;
use crate::runner::{BrokerOutcome, Floor, RunContext, ToolBroker};
use crate::snapshot::StepSnapshots;
use crate::store::{AuditEntry, RuleRow, Store, StoreError};
use crate::tools::{ToolRequest, ToolResult};
use next_agent_permission_engine::{
    always_rule, evaluate, explain_with, CommandClass, Context, Decision, Effect, Evaluation,
    FsFacts, Mode, Request, Rule, RuleSet, RuleTarget, Scope, Zone,
};
use std::path::{Path, PathBuf};

/// Real filesystem facts for the policy engine, so symlink and junction
/// escapes are judged against the disk rather than lexically.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl FsFacts for RealFs {
    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            std::fs::read_link(path).ok()
        } else {
            None
        }
    }
}

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

impl Approval {
    /// Wire label used in `approval_resolved` events and UI commands.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::AlwaysForWorkspace => "always_for_workspace",
            Self::Deny => "deny",
        }
    }

    /// Parse the wire label.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "allow" => Self::Allow,
            "always_for_workspace" => Self::AlwaysForWorkspace,
            "deny" => Self::Deny,
            _ => return None,
        })
    }

    /// Every choice, in the order the approval card shows them.
    pub const ALL: [Self; 3] = [Self::Allow, Self::Deny, Self::AlwaysForWorkspace];
}

/// Resolves `Decision::Ask`. In the app this becomes the approval card
/// ([`ChannelApprover`](crate::control::ChannelApprover)).
pub trait Approver {
    fn approve(&mut self, call_id: &str, request: &ToolRequest, reason: &str) -> Approval;

    /// Whether this approver emits its own `approval_requested` /
    /// `approval_resolved` events. When false, the broker records the
    /// resolution itself so no user decision is missing from the timeline.
    fn records_events(&self) -> bool {
        false
    }

    /// Who answers, for the audit log and `approval_resolved.by`: `user` for
    /// the approval card, or a setting such as `background-review`.
    fn answered_by(&self) -> &'static str {
        "user"
    }
}

impl Approver for Box<dyn Approver + Send + '_> {
    fn approve(&mut self, call_id: &str, request: &ToolRequest, reason: &str) -> Approval {
        (**self).approve(call_id, request, reason)
    }

    fn records_events(&self) -> bool {
        (**self).records_events()
    }

    fn answered_by(&self) -> &'static str {
        (**self).answered_by()
    }
}

/// Performs the side effect once policy has allowed it.
///
/// Deliberately has no policy logic: it must not re-check permissions, or the
/// two layers can disagree.
pub trait ToolExecutor {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult;

    /// Streaming form: may emit `tool_output` through `ctx.events` and must
    /// stop (killing any child process) once `ctx` is cancelled. Default:
    /// [`ToolExecutor::execute`].
    fn execute_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> ToolResult {
        let _ = ctx;
        self.execute(call_id, request)
    }
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
    snapshots: Option<Box<dyn StepSnapshots + 'a>>,
    untrusted: bool,
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
            snapshots: None,
            untrusted: false,
        }
    }

    /// The task comes from outside the desktop (a chat channel). Saved allow
    /// rules are ignored (deny rules still apply), "always for this
    /// workspace" answers count as allow-once and save nothing, and touching
    /// a secret or repository-control file (judged on the resolved path)
    /// needs an approval even where the mode would allow it.
    #[must_use]
    pub fn untrusted_origin(mut self) -> Self {
        self.untrusted = true;
        // Rules loaded before the flag was set may hold allow rules.
        self.rules.retain(|rule| rule.effect == Effect::Deny);
        self
    }

    /// Capture the work tree before each allowed mutating call (per-step
    /// undo). Requires [`SqliteBroker::for_task`] to record anything.
    #[must_use]
    pub fn with_snapshots(mut self, snapshots: Box<dyn StepSnapshots + 'a>) -> Self {
        self.snapshots = Some(snapshots);
        self
    }

    #[must_use]
    pub fn with_executor(mut self, executor: impl ToolExecutor + 'a) -> Self {
        self.executor = Box::new(executor);
        self
    }

    /// [`SqliteBroker::with_executor`] for an already-boxed executor.
    #[must_use]
    pub fn with_boxed_executor(mut self, executor: Box<dyn ToolExecutor + 'a>) -> Self {
        self.executor = executor;
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
    ///
    /// # Errors
    /// Returns [`StoreError`] if the rules cannot be read.
    pub fn load_rules(&mut self) -> Result<(), StoreError> {
        let rows = self.store.rules_for(&self.context.workspace_id)?;
        let untrusted = self.untrusted;
        self.rules = RuleSet::new(
            rows.iter()
                .filter_map(row_to_rule)
                .filter(|rule| !untrusted || rule.effect == Effect::Deny)
                .collect(),
        );
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
        self.handle_inner(None, call_id, request, None)
    }

    fn handle_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> BrokerOutcome {
        self.handle_inner(Some(ctx), call_id, request, None)
    }

    fn handle_floored(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
        floor: Option<&Floor>,
    ) -> BrokerOutcome {
        self.handle_inner(Some(ctx), call_id, request, floor)
    }
}

/// Apply an agent-profile floor on top of the engine's decision. Only ever
/// tightens: Deny wins over everything, Ask replaces Allow.
fn tighten(decision: Decision, floor: Option<&Floor>) -> Decision {
    match (floor, decision) {
        (None | Some(Floor::Ask(_)), decision @ (Decision::Deny { .. } | Decision::Ask { .. }))
        | (None, decision @ Decision::Allow { .. }) => decision,
        (Some(Floor::Deny(reason)), Decision::Deny { reason: engine }) => Decision::Deny {
            reason: format!("{engine}; {reason}"),
        },
        (Some(Floor::Deny(reason)), _) => Decision::Deny {
            reason: reason.clone(),
        },
        (Some(Floor::Ask(reason)), Decision::Allow { .. }) => Decision::Ask {
            reason: reason.clone(),
        },
    }
}

impl SqliteBroker<'_> {
    #[allow(clippy::too_many_lines)] // One decision path, kept in order.
    fn handle_inner(
        &mut self,
        ctx: Option<&RunContext<'_>>,
        call_id: &str,
        request: &ToolRequest,
        floor: Option<&Floor>,
    ) -> BrokerOutcome {
        let decided = |decision: &str, reason: &str| {
            if let Some(ctx) = ctx {
                ctx.events.emit(EventKind::PermissionDecided {
                    call_id: call_id.to_string(),
                    decision: decision.to_string(),
                    reason: reason.to_string(),
                });
            }
        };
        let policy_request = to_policy_request(request);
        let evaluation = explain_with(
            &policy_request,
            self.mode,
            &self.rules,
            &self.context,
            &RealFs,
        );
        // File tools act on the path the policy actually judged, so a link
        // cannot point the tool somewhere other than what was approved.
        let resolved_request = with_resolved_path(request, evaluation.resolved_path.as_deref());
        let request = &resolved_request;

        let decision = self.decide(&evaluation, floor);
        let reason = match decision {
            Decision::Allow { reason } => {
                decided("allow", &reason);
                reason
            }
            Decision::Deny { reason } => {
                decided("deny", &reason);
                self.audit(request, "deny", &reason);
                return BrokerOutcome {
                    decision: "deny",
                    reason: reason.clone(),
                    result: ToolResult::error(call_id, reason),
                };
            }
            Decision::Ask { reason } => {
                decided("ask", &reason);
                // Take the choice first: `approve` borrows the approver, and
                // the arms below need `&mut self` to persist or audit.
                let choice = self
                    .approver
                    .as_mut()
                    .map(|approver| approver.approve(call_id, request, &reason));
                self.record_answer(ctx, call_id, choice);
                let by = self
                    .approver
                    .as_ref()
                    .map_or("user", |approver| approver.answered_by());
                match choice {
                    Some(Approval::Allow) => {
                        self.audit(request, "allow", &format!("approved once by {by}"));
                        reason
                    }
                    Some(Approval::AlwaysForWorkspace) => {
                        self.approve_for_workspace(request, &policy_request, &evaluation);
                        reason
                    }
                    Some(Approval::Deny) => {
                        self.audit(request, "deny", &format!("{by} declined the approval"));
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

        let result = if let Some(ctx) = ctx {
            if ctx.is_cancelled() {
                return BrokerOutcome {
                    decision: "allow",
                    reason,
                    result: ToolResult::error(call_id, "cancelled before the tool started"),
                };
            }
            self.snapshot_before(Some(ctx), call_id, request);
            ctx.events.emit(EventKind::ToolStarted {
                call_id: call_id.to_string(),
            });
            self.executor.execute_in(ctx, call_id, request)
        } else {
            self.snapshot_before(None, call_id, request);
            self.executor.execute(call_id, request)
        };
        BrokerOutcome {
            decision: "allow",
            reason,
            result,
        }
    }
}

impl SqliteBroker<'_> {
    /// The engine's decision under the profile floor. For channel tasks an
    /// allowed touch of a secret or repository-control file (the engine's
    /// `protected` fact, judged on resolved paths and on paths a command
    /// mentions) becomes an approval.
    fn decide(&self, evaluation: &Evaluation, floor: Option<&Floor>) -> Decision {
        match tighten(evaluation.decision.clone(), floor) {
            Decision::Allow { .. } if self.untrusted && evaluation.protected => Decision::Ask {
                reason: "channel task touching a secret or repository-control file".into(),
            },
            decision => decision,
        }
    }

    /// Best-effort capture before an allowed mutating call. A failure is
    /// logged and never blocks the step.
    fn snapshot_before(
        &mut self,
        ctx: Option<&RunContext<'_>>,
        call_id: &str,
        request: &ToolRequest,
    ) {
        if !request.is_mutating() {
            return;
        }
        let (Some(snapshots), Some(task_id)) = (self.snapshots.as_mut(), self.task_id.as_deref())
        else {
            return;
        };
        let captured = match snapshots.capture() {
            Ok(captured) => captured,
            Err(error) => {
                tracing::warn!(%error, call_id, "step snapshot failed");
                return;
            }
        };
        match self.store.insert_step_snapshot(
            task_id,
            call_id,
            request.name(),
            &captured.work_tree,
            &captured.git_dir,
            &captured.tree,
        ) {
            Ok(seq) => {
                if let Some(ctx) = ctx {
                    ctx.events.emit(EventKind::StepSnapshot {
                        call_id: call_id.to_string(),
                        seq,
                        tree: captured.tree,
                    });
                }
            }
            Err(error) => tracing::warn!(%error, call_id, "step snapshot not recorded"),
        }
    }

    /// "Always for this workspace": persist a narrow rule when one exists,
    /// otherwise treat the answer as a one-time allow. Never a blanket rule.
    fn approve_for_workspace(
        &mut self,
        request: &ToolRequest,
        policy_request: &Request,
        evaluation: &Evaluation,
    ) {
        // Channel tasks never save rules: "always" counts as allow-once.
        let note = if self.untrusted {
            "approved once (no rule is saved for a channel task)"
        } else if self.persist_allow_rule(policy_request, evaluation) {
            "approved for workspace by user"
        } else {
            "approved once by user (no narrow workspace rule possible)"
        };
        self.audit(request, "allow", note);
    }

    /// Record the user's answer in the task timeline, not only the audit
    /// log, unless the approver already emits approval events itself.
    fn record_answer(&self, ctx: Option<&RunContext<'_>>, call_id: &str, choice: Option<Approval>) {
        let self_recording = self
            .approver
            .as_ref()
            .is_some_and(|approver| approver.records_events());
        if let (Some(ctx), Some(answer), false) = (ctx, choice, self_recording) {
            let by = self
                .approver
                .as_ref()
                .map_or("user", |approver| approver.answered_by());
            ctx.events.emit(EventKind::ApprovalResolved {
                approval_id: call_id.to_string(),
                choice: answer.as_str().to_string(),
                by: by.to_string(),
            });
        }
    }

    /// Persist the narrow rule for "Always for this workspace". Returns
    /// false when the engine refuses to derive one; the caller then treats
    /// the answer as a one-time allow.
    fn persist_allow_rule(&mut self, request: &Request, evaluation: &Evaluation) -> bool {
        let id_seed = format!("{request:?}");
        let id = format!(
            "allow-{}",
            &crate::hash::sha256_hex(id_seed.as_bytes())[..16]
        );
        let Some(rule) = always_rule(id, request, evaluation, &self.context.workspace_id) else {
            return false;
        };
        let (kind, pattern) = match &rule.target {
            RuleTarget::CommandPrefix { prefix } => {
                ("command", serde_json::to_string(prefix).unwrap_or_default())
            }
            RuleTarget::Path { glob } => ("path", glob.clone()),
            RuleTarget::Tool { .. } => return false,
        };
        let row = RuleRow {
            id: rule.id.clone(),
            scope: "workspace".into(),
            workspace_id: Some(self.context.workspace_id.clone()),
            effect: "allow".into(),
            kind: kind.into(),
            pattern,
        };
        if self.store.insert_rule(&row).is_err() {
            return false;
        }
        let _ = self.load_rules();
        true
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

/// Point file tools at the canonical path the policy evaluated.
fn with_resolved_path(request: &ToolRequest, resolved: Option<&Path>) -> ToolRequest {
    let Some(resolved) = resolved else {
        return request.clone();
    };
    let mut out = request.clone();
    match &mut out {
        ToolRequest::Read { path }
        | ToolRequest::ListDir { path }
        | ToolRequest::Write { path, .. }
        | ToolRequest::ApplyPatch { path, .. }
        | ToolRequest::Edit { path, .. } => *path = resolved.to_path_buf(),
        _ => {}
    }
    out
}

/// One-line target description used in audit rows, rules and approval cards.
#[must_use]
pub fn describe(request: &ToolRequest) -> String {
    match request {
        ToolRequest::Read { path }
        | ToolRequest::ListDir { path }
        | ToolRequest::Write { path, .. }
        | ToolRequest::ApplyPatch { path, .. }
        | ToolRequest::Edit { path, .. } => path.display().to_string(),
        ToolRequest::Search { pattern, path } => format!("{} in {}", pattern, path.display()),
        ToolRequest::Exec { command, .. } => command.clone(),
        ToolRequest::Network { url } => url.clone(),
        ToolRequest::AskUser { question } => question.clone(),
        ToolRequest::Learn(learn) => format!("{} {}", learn.tool, learn.target.display()),
    }
}

/// Map the tool layer's request onto the policy crate's dependency-free type.
#[must_use]
pub fn to_policy_request(request: &ToolRequest) -> Request {
    match request {
        ToolRequest::Read { path } => Request::Read { path: path.clone() },
        ToolRequest::ListDir { path } => Request::ListDir { path: path.clone() },
        ToolRequest::Search { path, .. } => Request::Search { path: path.clone() },
        ToolRequest::Write { path, .. }
        | ToolRequest::ApplyPatch { path, .. }
        | ToolRequest::Edit { path, .. } => Request::Write { path: path.clone() },
        ToolRequest::Exec { command, cwd } => Request::Exec {
            command: command.clone(),
            cwd: cwd.clone(),
        },
        ToolRequest::Network { url } => Request::Network { url: url.clone() },
        ToolRequest::AskUser { question } => Request::AskUser {
            question: question.clone(),
        },
        ToolRequest::Learn(learn) if learn.mutating => Request::Write {
            path: learn.target.clone(),
        },
        ToolRequest::Learn(learn) => Request::Read {
            path: learn.target.clone(),
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
        // Exact argv as a JSON array (written by "Always for this workspace");
        // hand-written rules may use plain whitespace-separated words.
        let prefix = if row.pattern.trim_start().starts_with('[') {
            serde_json::from_str::<Vec<String>>(&row.pattern).ok()?
        } else {
            row.pattern.split_whitespace().map(str::to_string).collect()
        };
        if prefix.is_empty() {
            return None;
        }
        RuleTarget::CommandPrefix { prefix }
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

    fn floored(
        broker: &mut SqliteBroker<'_>,
        request: &ToolRequest,
        floor: Option<&Floor>,
    ) -> (BrokerOutcome, Vec<crate::events::AgentEvent>) {
        let collector = crate::events::Collector::new();
        let events = crate::events::Emitter::new(crate::task::TaskId::from_seed("floor"))
            .with_sink(collector.clone());
        let never = || false;
        let ctx = RunContext {
            events: &events,
            cancelled: &never,
        };
        let outcome = broker.handle_floored(&ctx, "c1", request, floor);
        (outcome, collector.events())
    }

    #[test]
    fn profile_floor_only_tightens() {
        let store = Store::open_in_memory().expect("store");
        // Deny floor beats an engine allow.
        let mut auto = broker(&store, Mode::Auto);
        let (outcome, _) = floored(&mut auto, &read(), Some(&Floor::Deny("profile".into())));
        assert_eq!(outcome.decision, "deny");
        assert!(!outcome.result.ok);
        // Ask floor turns an allow into an approval (approved once here).
        let mut asking = broker(&store, Mode::Auto).with_approver(Scripted(Approval::Allow));
        let (outcome, events) = floored(
            &mut asking,
            &read(),
            Some(&Floor::Ask("profile asks".into())),
        );
        assert_eq!(outcome.decision, "allow");
        assert!(events.iter().any(|e| e.data()["decision"] == "ask"));
        // ...and fails closed with no approver.
        let mut silent = broker(&store, Mode::Auto);
        let (outcome, _) = floored(&mut silent, &read(), Some(&Floor::Ask("p".into())));
        assert!(!outcome.result.ok);
        // A floor can never loosen an engine deny.
        let mut bypass = broker(&store, Mode::BypassPermissions);
        let rm = ToolRequest::Exec {
            command: "rm -rf /".into(),
            cwd: "/work/project".into(),
        };
        let (outcome, _) = floored(&mut bypass, &rm, Some(&Floor::Ask("p".into())));
        assert_eq!(outcome.decision, "deny");
        // Tightening table.
        let allow = || Decision::Allow { reason: "a".into() };
        assert!(matches!(tighten(allow(), None), Decision::Allow { .. }));
        assert!(matches!(
            tighten(
                Decision::Ask { reason: "x".into() },
                Some(&Floor::Deny("d".into()))
            ),
            Decision::Deny { .. }
        ));
    }

    struct CountingSnapshots(std::rc::Rc<std::cell::Cell<usize>>);
    impl StepSnapshots for CountingSnapshots {
        fn capture(&mut self) -> Result<crate::snapshot::CapturedStep, String> {
            self.0.set(self.0.get() + 1);
            Ok(crate::snapshot::CapturedStep {
                work_tree: "/work/project".into(),
                git_dir: "/data/snapshot/x".into(),
                tree: format!("tree{}", self.0.get()),
            })
        }
    }

    #[test]
    fn mutating_allowed_calls_are_snapshotted_first() {
        let store = Store::open_in_memory().expect("store");
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut broker = SqliteBroker::new(
            &store,
            Mode::BypassPermissions,
            context_for("/work/project".into(), "ws_1".into(), None),
        )
        .with_executor(StubExecutor)
        .with_snapshots(Box::new(CountingSnapshots(count.clone())))
        .for_task("t1");
        let write = ToolRequest::Write {
            path: "/work/project/a.rs".into(),
            contents: "x".into(),
        };
        let (outcome, events) = floored(&mut broker, &write, None);
        assert!(outcome.result.ok);
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.kind()).collect();
        assert_eq!(
            kinds,
            ["permission_decided", "step_snapshot", "tool_started"]
        );
        // Reads and denied calls are not snapshotted.
        let _ = floored(&mut broker, &read(), None);
        let rm = ToolRequest::Exec {
            command: "rm -rf /".into(),
            cwd: "/work/project".into(),
        };
        let _ = floored(&mut broker, &rm, None);
        assert_eq!(count.get(), 1);
        let rows = store.step_snapshots("t1").expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool, "write_file");
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

    fn exec(command: &str, cwd: &std::path::Path) -> ToolRequest {
        ToolRequest::Exec {
            command: command.into(),
            cwd: cwd.to_path_buf(),
        }
    }

    /// A registered workspace (rules reference it by foreign key).
    fn registered(tag: &str) -> (Store, crate::Workspace) {
        let dir = std::env::temp_dir().join(format!("na-always-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let workspace = crate::Workspace::open(&dir).expect("workspace");
        let store = Store::open_in_memory().expect("store");
        store.upsert_workspace(&workspace).expect("register");
        (store, workspace)
    }

    fn answer_with<'a>(
        store: &'a Store,
        workspace: &crate::Workspace,
        approval: Approval,
    ) -> SqliteBroker<'a> {
        let context = context_for(
            workspace.root().to_path_buf(),
            workspace.id().to_string(),
            None,
        );
        let mut b = SqliteBroker::new(store, Mode::Manual, context)
            .with_executor(StubExecutor)
            .with_approver(Scripted(approval));
        b.load_rules().expect("load");
        b
    }

    #[test]
    fn always_for_workspace_allows_only_the_exact_command() {
        let (store, ws) = registered("exact");
        let root = ws.root();
        let first = answer_with(&store, &ws, Approval::AlwaysForWorkspace)
            .handle("c1", &exec("pnpm test", root));
        assert_eq!(first.decision, "allow");

        let rules = store.rules_for(ws.id()).expect("rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, "command");

        // Same command: the stored rule allows it without asking.
        let again = answer_with(&store, &ws, Approval::Deny).handle("c2", &exec("pnpm test", root));
        assert_eq!(again.decision, "allow");

        // A different command must still ask (and the user can decline it).
        let other =
            answer_with(&store, &ws, Approval::Deny).handle("c3", &exec("cargo build", root));
        assert_eq!(other.decision, "deny", "always-rule must not be tool-wide");
    }

    #[test]
    fn always_for_workspace_on_compound_command_is_one_time() {
        let (store, ws) = registered("compound");
        let outcome = answer_with(&store, &ws, Approval::AlwaysForWorkspace)
            .handle("c1", &exec("pnpm test && pnpm build", ws.root()));
        assert_eq!(outcome.decision, "allow");
        assert_eq!(store.rules_for(ws.id()).expect("rules").len(), 0);
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
                kind: "command".into(),
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

    fn allow_writes(store: &Store) {
        store
            .insert_rule(&RuleRow {
                id: "allow-writes".into(),
                scope: "global".into(),
                workspace_id: None,
                effect: "allow".into(),
                kind: "write_file".into(),
                pattern: String::new(),
            })
            .expect("rule");
    }

    fn write_req() -> ToolRequest {
        ToolRequest::Write {
            path: "/work/project/a.txt".into(),
            contents: "x".into(),
        }
    }

    #[test]
    fn untrusted_origin_ignores_saved_allow_rules_whatever_the_call_order() {
        let store = Store::open_in_memory().expect("store");
        allow_writes(&store);
        // Trusted: the saved rule upgrades the ask.
        let mut trusted = broker(&store, Mode::Manual);
        trusted.load_rules().expect("load");
        assert_eq!(trusted.handle("c1", &write_req()).decision, "allow");
        // Untrusted, flag set after rules were loaded: still ignored.
        let mut late = broker(&store, Mode::Manual);
        late.load_rules().expect("load");
        let mut late = late.untrusted_origin();
        assert_eq!(late.handle("c2", &write_req()).decision, "ask");
        // Untrusted, flag set first.
        let mut early = broker(&store, Mode::Manual).untrusted_origin();
        early.load_rules().expect("load");
        assert_eq!(early.handle("c3", &write_req()).decision, "ask");
    }

    #[test]
    fn untrusted_always_answer_saves_no_rule() {
        let store = Store::open_in_memory().expect("store");
        let mut broker = broker(&store, Mode::Manual)
            .untrusted_origin()
            .with_approver(Scripted(Approval::AlwaysForWorkspace));
        assert_eq!(broker.handle("c1", &write_req()).decision, "allow");
        assert_eq!(store.rules_for("ws_1").expect("rules").len(), 0);
    }

    #[test]
    fn untrusted_reads_of_secret_files_ask() {
        let store = Store::open_in_memory().expect("store");
        let mut broker = broker(&store, Mode::Manual).untrusted_origin();
        let secret = ToolRequest::Read {
            path: "/work/project/.env".into(),
        };
        assert_eq!(broker.handle("c1", &secret).decision, "ask");
        assert_eq!(broker.handle("c2", &read()).decision, "allow");
        let mut trusted = super::tests::broker(&store, Mode::Manual);
        assert_eq!(trusted.handle("c3", &secret).decision, "allow");
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
