//! The decision function. Pure: same inputs, same output, no I/O.
//!
//! Order (first match wins) — see `docs/architecture/Permission Engine.md`:
//! 1. user `deny` rule → deny, in every mode
//! 2. catastrophic command → deny, in every mode
//! 3. `ask_user` → always ask
//! 4. mode policy table
//! 5. user `allow` rule upgrades an `ask` to `allow` (never overrides a deny,
//!    and never permits a mutation in `plan`)

use crate::command::{classify, CommandClass};
use crate::decision::Decision;
use crate::mode::Mode;
use crate::path::{self, Sandbox, Zone};
use crate::rules::{Effect, RuleSet, Subject};
use std::path::PathBuf;

/// A side-effecting request, expressed independently of `core`'s tool types so
/// this crate stays dependency-free. The broker maps `ToolRequest` into this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Read { path: PathBuf },
    ListDir { path: PathBuf },
    Search { path: PathBuf },
    Write { path: PathBuf },
    Exec { command: String, cwd: PathBuf },
    Network { url: String },
    AskUser { question: String },
}

impl Request {
    /// Tool name, matching `core::ToolRequest::kind`.
    #[must_use]
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Read { .. } => "read_file",
            Self::ListDir { .. } => "list_dir",
            Self::Search { .. } => "search",
            Self::Write { .. } => "write_file",
            Self::Exec { .. } => "run_command",
            Self::Network { .. } => "http_fetch",
            Self::AskUser { .. } => "ask_user",
        }
    }

    #[must_use]
    pub fn is_mutating(&self) -> bool {
        matches!(self, Self::Write { .. } | Self::Exec { .. })
    }

    fn own_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Read { path }
            | Self::ListDir { path }
            | Self::Search { path }
            | Self::Write { path } => Some(path),
            Self::Exec { .. } | Self::Network { .. } | Self::AskUser { .. } => None,
        }
    }
}

/// Everything the decision depends on.
#[derive(Debug, Clone)]
pub struct Context {
    pub sandbox: Sandbox,
    /// Stable workspace id used to scope rules.
    pub workspace_id: String,
    /// Case-insensitive path comparison (macOS, Windows default volumes).
    pub case_insensitive: bool,
    /// Hosts the agent may reach without asking in `auto`.
    pub allowlisted_hosts: Vec<String>,
}

/// Outcome plus the facts worth logging in the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub decision: Decision,
    /// Short machine-readable label for `permission_decided` events.
    pub rule: &'static str,
    pub class: Option<CommandClass>,
    pub zone: Option<Zone>,
}

/// Evaluate one request against the active mode and rule set.
#[must_use]
pub fn evaluate(request: &Request, mode: Mode, rules: &RuleSet, ctx: &Context) -> Decision {
    explain(request, mode, rules, ctx).decision
}

/// Same as [`evaluate`] but keeps the reasoning for the event log.
#[must_use]
pub fn explain(request: &Request, mode: Mode, rules: &RuleSet, ctx: &Context) -> Evaluation {
    let resolved = request.own_path().map(|path| resolve(path, request, ctx));
    let zone = resolved
        .as_ref()
        .map(|path| path::zone(path, &ctx.sandbox, ctx.case_insensitive));
    let class = match request {
        Request::Exec { command, .. } => Some(classify(command)),
        _ => None,
    };
    let subject = Subject {
        tool: request.tool(),
        path: resolved.as_ref().and_then(|path| path.to_str()),
        command: match request {
            Request::Exec { command, .. } => Some(command.as_str()),
            _ => None,
        },
    };

    let build = |decision: Decision, rule: &'static str| Evaluation {
        decision,
        rule,
        class,
        zone,
    };

    // 1. Explicit user denial, in every mode including bypass.
    if let Some(rule) = rules.matching(Effect::Deny, &subject, &ctx.workspace_id) {
        return build(
            Decision::deny(format!("denied by rule {}", rule.id)),
            "user_deny",
        );
    }

    // 2. Catastrophic commands are never allowed.
    if class == Some(CommandClass::Catastrophic) {
        return build(
            Decision::deny("catastrophic command; refused in every mode"),
            "catastrophic",
        );
    }

    // 3. A question to the user is itself the request.
    if matches!(request, Request::AskUser { .. }) {
        return build(Decision::ask("agent asked a question"), "ask_user");
    }

    // 4. Mode policy.
    let (policy, reason) = mode_policy(request, mode, zone, class, ctx);

    // 5. An allow rule may upgrade an ask — never a deny, never a plan mutation.
    if policy == Polarity::Ask && !(mode.is_read_only() && request.is_mutating()) {
        if let Some(rule) = rules.matching(Effect::Allow, &subject, &ctx.workspace_id) {
            return build(
                Decision::allow(format!("allowed by rule {}", rule.id)),
                "user_allow",
            );
        }
    }

    let rule = reason_rule(mode, policy);
    build(
        match policy {
            Polarity::Allow => Decision::allow(reason),
            Polarity::Ask => Decision::ask(reason),
            Polarity::Deny => Decision::deny(reason),
        },
        rule,
    )
}

fn resolve(path: &std::path::Path, request: &Request, ctx: &Context) -> PathBuf {
    let cwd = match request {
        Request::Exec { cwd, .. } => cwd.clone(),
        _ => ctx.sandbox.root().to_path_buf(),
    };
    ctx.sandbox.resolve(&cwd, path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Allow,
    Ask,
    Deny,
}

fn reason_rule(mode: Mode, polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Allow => match mode {
            Mode::Plan => "plan_readonly",
            Mode::Manual => "mode_allow",
            Mode::Auto => "auto_sandbox",
            Mode::BypassPermissions => "bypass",
        },
        Polarity::Ask => "mode_ask",
        Polarity::Deny => match mode {
            Mode::Plan => "plan_denied",
            Mode::Auto => "auto_forbidden",
            _ => "mode_denied",
        },
    }
}

fn mode_policy(
    request: &Request,
    mode: Mode,
    zone: Option<Zone>,
    class: Option<CommandClass>,
    ctx: &Context,
) -> (Polarity, String) {
    let zone = zone.unwrap_or(Zone::Inside);

    if mode == Mode::BypassPermissions {
        return (
            Polarity::Allow,
            "bypass mode: action allowed and audited".into(),
        );
    }

    match request {
        Request::Read { .. } | Request::ListDir { .. } | Request::Search { .. } => match zone {
            Zone::Inside => (Polarity::Allow, "read inside workspace".into()),
            Zone::Outside => (Polarity::Ask, "read outside workspace".into()),
            Zone::Sensitive => {
                if mode == Mode::Plan {
                    (Polarity::Deny, "sensitive path in plan mode".into())
                } else {
                    (Polarity::Ask, "read of a sensitive path".into())
                }
            }
        },
        Request::Write { path } => {
            if mode == Mode::Plan {
                return (Polarity::Deny, "plan mode does not write".into());
            }
            if zone == Zone::Sensitive {
                return (Polarity::Deny, "write to a sensitive path".into());
            }
            let secret = path::is_secret_file(path);
            match (mode, zone) {
                (Mode::Auto, Zone::Inside) if secret => {
                    (Polarity::Ask, "write to a secret file".into())
                }
                (Mode::Auto, Zone::Inside) => (Polarity::Allow, "write inside the worktree".into()),
                (Mode::Auto, _) => (Polarity::Ask, "write outside the workspace".into()),
                (_, Zone::Inside) => (Polarity::Ask, "write inside workspace".into()),
                (_, _) => (Polarity::Ask, "write outside workspace".into()),
            }
        }
        Request::Exec { .. } => {
            let class = class.unwrap_or(CommandClass::Unknown);
            if mode == Mode::Plan {
                return (
                    Polarity::Deny,
                    format!("plan mode blocks commands ({})", class.label()),
                );
            }
            match class {
                CommandClass::SafeDev => (Polarity::Allow, "safe development command".into()),
                CommandClass::Unknown => (Polarity::Ask, "unclassified command".into()),
                other if mode == Mode::Auto => (
                    Polarity::Deny,
                    format!("{} commands are blocked in auto", other.label()),
                ),
                other => (
                    Polarity::Ask,
                    format!("{} command needs approval", other.label()),
                ),
            }
        }
        Request::Network { url } => {
            if mode == Mode::Plan {
                return (Polarity::Deny, "plan mode does not use the network".into());
            }
            if mode == Mode::Auto && host_allowlisted(url, &ctx.allowlisted_hosts) {
                return (Polarity::Allow, "allowlisted host".into());
            }
            (Polarity::Ask, "outbound request".into())
        }
        Request::AskUser { .. } => (Polarity::Ask, "agent asked a question".into()),
    }
}

fn host_allowlisted(url: &str, hosts: &[String]) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port.split(':').next().unwrap_or(host_port);
    hosts.iter().any(|allowed| allowed == host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Rule, RuleTarget, Scope};

    fn ctx() -> Context {
        Context {
            sandbox: Sandbox::new("/work/project", Some(PathBuf::from("/home/dev"))),
            workspace_id: "ws_1".into(),
            case_insensitive: false,
            allowlisted_hosts: vec!["docs.rs".into()],
        }
    }

    fn read(path: &str) -> Request {
        Request::Read {
            path: PathBuf::from(path),
        }
    }

    fn write(path: &str) -> Request {
        Request::Write {
            path: PathBuf::from(path),
        }
    }

    fn exec(command: &str) -> Request {
        Request::Exec {
            command: command.into(),
            cwd: PathBuf::from("/work/project"),
        }
    }

    #[test]
    fn reads_inside_are_allowed_in_every_mode() {
        for mode in [
            Mode::Plan,
            Mode::Manual,
            Mode::Auto,
            Mode::BypassPermissions,
        ] {
            assert!(evaluate(&read("src/lib.rs"), mode, &RuleSet::empty(), &ctx()).is_allowed());
        }
    }

    #[test]
    fn reads_outside_ask_except_bypass() {
        assert!(matches!(
            evaluate(
                &read("/home/dev/notes.md"),
                Mode::Manual,
                &RuleSet::empty(),
                &ctx()
            ),
            Decision::Ask { .. }
        ));
        assert!(evaluate(
            &read("/home/dev/notes.md"),
            Mode::BypassPermissions,
            &RuleSet::empty(),
            &ctx()
        )
        .is_allowed());
    }

    #[test]
    fn plan_never_writes_or_executes() {
        assert!(evaluate(&write("src/a.rs"), Mode::Plan, &RuleSet::empty(), &ctx()).is_denied());
        assert!(evaluate(&exec("pnpm test"), Mode::Plan, &RuleSet::empty(), &ctx()).is_denied());
    }

    #[test]
    fn auto_writes_inside_and_asks_outside() {
        assert!(evaluate(&write("src/a.rs"), Mode::Auto, &RuleSet::empty(), &ctx()).is_allowed());
        assert!(matches!(
            evaluate(&write("/tmp/a.rs"), Mode::Auto, &RuleSet::empty(), &ctx()),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn auto_asks_for_dotenv_writes() {
        assert!(matches!(
            evaluate(&write(".env"), Mode::Auto, &RuleSet::empty(), &ctx()),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn auto_blocks_sensitive_and_network_commands() {
        for command in [
            "sudo apt install x",
            "ssh host",
            "git push origin main",
            "terraform apply",
        ] {
            assert!(
                evaluate(&exec(command), Mode::Auto, &RuleSet::empty(), &ctx()).is_denied(),
                "{command} must be denied in auto"
            );
        }
    }

    #[test]
    fn manual_allows_safe_commands_and_asks_for_the_rest() {
        assert!(evaluate(&exec("pnpm test"), Mode::Manual, &RuleSet::empty(), &ctx()).is_allowed());
        assert!(matches!(
            evaluate(
                &exec("sudo apt install x"),
                Mode::Manual,
                &RuleSet::empty(),
                &ctx()
            ),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn catastrophic_is_denied_in_every_mode_including_bypass() {
        for mode in [
            Mode::Plan,
            Mode::Manual,
            Mode::Auto,
            Mode::BypassPermissions,
        ] {
            assert!(
                evaluate(&exec("rm -rf /"), mode, &RuleSet::empty(), &ctx()).is_denied(),
                "{mode:?} allowed rm -rf /"
            );
        }
    }

    #[test]
    fn deny_rule_beats_everything() {
        let rules = RuleSet::new(vec![Rule::deny(
            "no-tests",
            RuleTarget::CommandPrefix {
                prefix: vec!["pnpm".into(), "test".into()],
            },
        )]);
        for mode in [Mode::Manual, Mode::Auto, Mode::BypassPermissions] {
            assert!(evaluate(&exec("pnpm test"), mode, &rules, &ctx()).is_denied());
        }
    }

    #[test]
    fn allow_rule_upgrades_ask_but_not_a_plan_mutation() {
        let rules = RuleSet::new(vec![Rule::allow(
            "read-home",
            RuleTarget::Path {
                glob: "/home/dev/**".into(),
            },
        )]);
        assert!(evaluate(&read("/home/dev/notes.md"), Mode::Manual, &rules, &ctx()).is_allowed());

        let write_rules = RuleSet::new(vec![Rule::allow(
            "w",
            RuleTarget::Tool {
                kind: "write_file".into(),
            },
        )]);
        assert!(evaluate(&write("a.rs"), Mode::Plan, &write_rules, &ctx()).is_denied());
    }

    #[test]
    fn ask_user_always_asks_even_in_bypass() {
        let request = Request::AskUser {
            question: "which branch?".into(),
        };
        for mode in [
            Mode::Plan,
            Mode::Manual,
            Mode::Auto,
            Mode::BypassPermissions,
        ] {
            assert!(matches!(
                evaluate(&request, mode, &RuleSet::empty(), &ctx()),
                Decision::Ask { .. }
            ));
        }
    }

    #[test]
    fn network_allowlist_only_applies_in_auto() {
        let allowed = Request::Network {
            url: "https://docs.rs/foo".into(),
        };
        let other = Request::Network {
            url: "https://example.com".into(),
        };
        assert!(evaluate(&allowed, Mode::Auto, &RuleSet::empty(), &ctx()).is_allowed());
        assert!(matches!(
            evaluate(&other, Mode::Auto, &RuleSet::empty(), &ctx()),
            Decision::Ask { .. }
        ));
        assert!(matches!(
            evaluate(&allowed, Mode::Manual, &RuleSet::empty(), &ctx()),
            Decision::Ask { .. }
        ));
        assert!(evaluate(&allowed, Mode::Plan, &RuleSet::empty(), &ctx()).is_denied());
    }

    #[test]
    fn nested_escape_is_not_allowed() {
        let escape = read("../../etc/passwd");
        assert!(!evaluate(&escape, Mode::Auto, &RuleSet::empty(), &ctx()).is_allowed());
    }

    #[test]
    fn workspace_scoped_rules_are_ignored_elsewhere() {
        let rules = RuleSet::new(vec![Rule::allow(
            "w",
            RuleTarget::Tool {
                kind: "write_file".into(),
            },
        )
        .scoped_to(Scope::Workspace {
            id: "ws_other".into(),
        })]);
        assert!(matches!(
            evaluate(&write("/tmp/a.rs"), Mode::Auto, &rules, &ctx()),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn explain_reports_the_rule_and_facts() {
        let evaluation = explain(&exec("rm -rf /"), Mode::Auto, &RuleSet::empty(), &ctx());
        assert_eq!(evaluation.rule, "catastrophic");
        assert_eq!(evaluation.class, Some(CommandClass::Catastrophic));

        let allowed = explain(&exec("pnpm test"), Mode::Auto, &RuleSet::empty(), &ctx());
        assert_eq!(allowed.rule, "auto_sandbox");
        assert_eq!(allowed.class, Some(CommandClass::SafeDev));
    }

    #[test]
    fn request_metadata_matches_core_tool_names() {
        assert_eq!(read("a").tool(), "read_file");
        assert_eq!(write("a").tool(), "write_file");
        assert_eq!(exec("ls").tool(), "run_command");
        assert!(exec("ls").is_mutating());
        assert!(!read("a").is_mutating());
    }
}
