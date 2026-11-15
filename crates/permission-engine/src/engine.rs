//! The decision function. Pure: same inputs, same output, no I/O.
//!
//! Order (first match wins) — see `docs/architecture/Permission Engine.md`:
//! 1. user `deny` rule → deny, in every mode
//! 2. catastrophic command → deny, in every mode
//! 3. `ask_user` → always ask
//! 4. mode policy table
//! 5. user `allow` rule upgrades an `ask` to `allow` (never overrides a deny,
//!    never permits a mutation in `plan`, and only an explicit path rule can
//!    unlock a sensitive path)

use crate::command::{analyze, Analysis, CommandClass};
use crate::decision::Decision;
use crate::mode::Mode;
use crate::path::{FsFacts, Lexical, PathStyle, Sandbox, Zone};
use crate::rules::{escape_glob, Effect, GlobEnv, Rule, RuleSet, RuleTarget, Scope, Subject};
use std::path::{Path, PathBuf};

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

    fn own_path(&self) -> Option<&Path> {
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
    /// Windows-style sandboxes are always case-insensitive.
    pub case_insensitive: bool,
    /// Hosts the agent may reach without asking in `auto` (`http`/`https` only).
    pub allowlisted_hosts: Vec<String>,
}

/// Outcome plus the facts worth logging in the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub decision: Decision,
    /// Short machine-readable label for `permission_decided` events.
    pub rule: &'static str,
    pub class: Option<CommandClass>,
    /// For file tools, the target's zone; for commands, the most restrictive
    /// zone of the working directory and every path the command mentions.
    pub zone: Option<Zone>,
    /// Canonical target of a file tool. The broker must operate on exactly
    /// this path (and re-check it at open time) so the decision and the side
    /// effect cannot diverge through `..` or symlink tricks.
    pub resolved_path: Option<PathBuf>,
    /// The target (or, for commands, a path the command mentions) is a secret
    /// file or repository control dir, judged on the resolved path. Callers
    /// with stricter rules (channel tasks) use it to ask where the mode would
    /// allow.
    pub protected: bool,
}

/// Evaluate one request against the active mode and rule set, with lexical
/// path resolution only.
#[must_use]
pub fn evaluate(request: &Request, mode: Mode, rules: &RuleSet, ctx: &Context) -> Decision {
    explain(request, mode, rules, ctx).decision
}

/// Same as [`evaluate`] but keeps the reasoning for the event log.
#[must_use]
pub fn explain(request: &Request, mode: Mode, rules: &RuleSet, ctx: &Context) -> Evaluation {
    explain_with(request, mode, rules, ctx, &Lexical)
}

/// Full evaluation with filesystem facts (symlinks, junctions, aliases like
/// `/tmp` → `/private/tmp`). The broker should call this with a real
/// [`FsFacts`] implementation.
#[must_use]
pub fn explain_with(
    request: &Request,
    mode: Mode,
    rules: &RuleSet,
    ctx: &Context,
    fs: &dyn FsFacts,
) -> Evaluation {
    let facts = gather(request, ctx, fs);
    let globs = glob_env(ctx, fs);
    let subject = Subject {
        tool: request.tool(),
        own_paths: &facts.own_paths,
        mentioned_paths: &facts.mentioned_paths,
        commands: facts
            .analysis
            .as_ref()
            .map_or(&[][..], |analysis| &analysis.commands[..]),
        opaque: facts
            .analysis
            .as_ref()
            .is_some_and(|analysis| analysis.opaque),
        globs: &globs,
    };
    let class = facts.analysis.as_ref().map(|analysis| analysis.class);
    let zone = facts.zone;
    let resolved_path = facts.resolved.clone();
    let protected = facts.protected;

    let build = |decision: Decision, rule: &'static str| Evaluation {
        decision,
        rule,
        class,
        zone,
        resolved_path: resolved_path.clone(),
        protected,
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
    let (policy, reason) = mode_policy(request, mode, &facts, ctx);

    // 5. An allow rule may upgrade an ask — never a deny, never a plan
    //    mutation, and a sensitive path only through an explicit path rule.
    if policy == Polarity::Ask && !(mode.is_read_only() && request.is_mutating()) {
        let sensitive = zone == Some(Zone::Sensitive);
        let file_tool = request.own_path().is_some();
        if let Some(rule) = rules
            .matching(Effect::Allow, &subject, &ctx.workspace_id)
            .filter(|rule| {
                !sensitive || (file_tool && matches!(rule.target, RuleTarget::Path { .. }))
            })
        {
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

/// Facts derived from the request before any policy is applied.
struct Facts {
    zone: Option<Zone>,
    resolved: Option<PathBuf>,
    /// Secret file (`.env*`) or repository control dir (`.git`, `.next-agent`).
    protected: bool,
    /// Secret file only (reads of `.git` are fine).
    secret: bool,
    note: Option<&'static str>,
    own_paths: Vec<String>,
    mentioned_paths: Vec<String>,
    analysis: Option<Analysis>,
}

fn gather(request: &Request, ctx: &Context, fs: &dyn FsFacts) -> Facts {
    let checker = ctx.sandbox.checker(ctx.case_insensitive, fs);
    let mut facts = Facts {
        zone: None,
        resolved: None,
        protected: false,
        secret: false,
        note: None,
        own_paths: Vec::new(),
        mentioned_paths: Vec::new(),
        analysis: None,
    };
    if let Some(path) = request.own_path() {
        let check = checker.check(None, &path.to_string_lossy());
        facts.zone = Some(check.zone);
        facts.note = check.note;
        if let Some(resolved) = &check.resolved {
            facts.protected = checker.is_protected(resolved);
            facts.secret = checker.is_secret(resolved);
            facts
                .own_paths
                .push(resolved.to_string_lossy().into_owned());
        }
        facts.resolved = check.resolved;
    }
    if let Request::Exec { command, cwd } = request {
        let analysis = analyze(command);
        let cwd_check = checker.check(None, &cwd.to_string_lossy());
        let mut zone = cwd_check.zone;
        facts.note = cwd_check.note;
        let base = cwd_check.resolved.clone();
        if let Some(resolved) = &cwd_check.resolved {
            facts
                .mentioned_paths
                .push(resolved.to_string_lossy().into_owned());
        }
        for arg in &analysis.paths {
            let check = checker.check_shell_word(base.as_deref(), &arg.text);
            if check.zone > zone {
                zone = check.zone;
                facts.note = check.note;
            }
            if let Some(resolved) = &check.resolved {
                if checker.is_protected(resolved) {
                    facts.protected = true;
                }
                facts
                    .mentioned_paths
                    .push(resolved.to_string_lossy().into_owned());
            }
        }
        facts.zone = Some(zone);
        facts.analysis = Some(analysis);
    }
    facts
}

fn glob_env(ctx: &Context, fs: &dyn FsFacts) -> GlobEnv {
    let checker = ctx.sandbox.checker(ctx.case_insensitive, fs);
    let windows = ctx.sandbox.style() == PathStyle::Windows;
    GlobEnv {
        home: ctx
            .sandbox
            .home()
            .and_then(|home| checker.check(None, &home.to_string_lossy()).resolved)
            .map(|home| home.to_string_lossy().into_owned()),
        root: checker
            .root_path()
            .map(|root| root.to_string_lossy().into_owned()),
        backslash_separator: windows,
        case_insensitive: ctx.case_insensitive || windows,
    }
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

fn mode_policy(request: &Request, mode: Mode, facts: &Facts, ctx: &Context) -> (Polarity, String) {
    if mode == Mode::BypassPermissions {
        return (
            Polarity::Allow,
            "bypass mode: action allowed and audited".into(),
        );
    }
    let zone = facts.zone.unwrap_or(Zone::Sensitive);
    let note = facts.note.unwrap_or("sensitive path");

    match request {
        Request::Read { .. } | Request::ListDir { .. } | Request::Search { .. } => match zone {
            Zone::Inside if mode == Mode::Auto && facts.secret => {
                (Polarity::Ask, "read of a secret file".into())
            }
            Zone::Inside => (Polarity::Allow, "read inside workspace".into()),
            Zone::Outside => (Polarity::Ask, "read outside workspace".into()),
            Zone::Sensitive if mode == Mode::Auto => {
                (Polarity::Deny, format!("auto never reads here: {note}"))
            }
            Zone::Sensitive => (Polarity::Ask, format!("read of a sensitive path: {note}")),
        },
        Request::Write { .. } => match (mode, zone) {
            (Mode::Plan, _) => (Polarity::Deny, "plan mode does not write".into()),
            (Mode::Auto, Zone::Inside) if facts.protected => (
                Polarity::Ask,
                "write to a secret or repository-control file".into(),
            ),
            (Mode::Auto, Zone::Inside) => (Polarity::Allow, "write inside the worktree".into()),
            (Mode::Auto, Zone::Outside) => (Polarity::Ask, "write outside the workspace".into()),
            (Mode::Auto, Zone::Sensitive) => {
                (Polarity::Deny, format!("auto never writes here: {note}"))
            }
            (_, Zone::Inside) => (Polarity::Ask, "write inside workspace".into()),
            (_, Zone::Outside) => (Polarity::Ask, "write outside workspace".into()),
            (_, Zone::Sensitive) => (Polarity::Ask, format!("write to a sensitive path: {note}")),
        },
        Request::Exec { .. } => {
            let class = facts
                .analysis
                .as_ref()
                .map_or(CommandClass::Unknown, |analysis| analysis.class);
            match mode {
                Mode::Plan => (
                    Polarity::Deny,
                    format!("plan mode blocks commands ({})", class.label()),
                ),
                Mode::Auto if class.auto_forbidden() => (
                    Polarity::Deny,
                    format!("{} commands are blocked in auto", class.label()),
                ),
                Mode::Auto if zone == Zone::Sensitive => (
                    Polarity::Deny,
                    format!("command touches a sensitive path: {note}"),
                ),
                Mode::Auto if class == CommandClass::SafeDev && zone == Zone::Inside => {
                    if facts.protected {
                        (
                            Polarity::Ask,
                            "command touches a secret or repository-control file".into(),
                        )
                    } else {
                        (Polarity::Allow, "safe development command".into())
                    }
                }
                Mode::Auto if class == CommandClass::SafeDev => (
                    Polarity::Ask,
                    "command reaches outside the workspace".into(),
                ),
                _ => (
                    Polarity::Ask,
                    format!("{} command needs approval", class.label()),
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

/// Strict allowlist check. Anything a URL parser could read differently
/// (userinfo, backslashes, whitespace, IPv6 literals, odd schemes) is refused
/// rather than interpreted.
#[must_use]
pub fn host_allowlisted(url: &str, hosts: &[String]) -> bool {
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\' || c == '%')
    {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') || authority.starts_with('[') {
        return false;
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        Some(_) => return false,
        None => authority,
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
}

/// The rule to persist when the user answers an approval with "Always for
/// this workspace". Deliberately narrow:
/// - file tools: the exact canonical path (glob-escaped), for that tool's
///   kind of access only through the path;
/// - commands: the exact argv of a single simple command;
/// - anything else (compound commands, unparseable lines, network, questions):
///   `None` — approve once instead of minting a blanket rule.
#[must_use]
pub fn always_rule(
    id: impl Into<String>,
    request: &Request,
    evaluation: &Evaluation,
    workspace_id: &str,
) -> Option<Rule> {
    let scope = Scope::Workspace {
        id: workspace_id.to_string(),
    };
    let target = match request {
        Request::Read { .. }
        | Request::ListDir { .. }
        | Request::Search { .. }
        | Request::Write { .. } => RuleTarget::Path {
            glob: escape_glob(&evaluation.resolved_path.as_ref()?.to_string_lossy()),
        },
        Request::Exec { command, .. } => {
            let analysis = analyze(command);
            if analysis.opaque
                || analysis.commands.len() != 1
                || analysis.class == CommandClass::Catastrophic
            {
                return None;
            }
            RuleTarget::CommandPrefix {
                prefix: analysis.commands.into_iter().next()?,
            }
        }
        Request::Network { .. } | Request::AskUser { .. } => return None,
    };
    Some(Rule::allow(id, target).scoped_to(scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::MapFs;

    fn ctx() -> Context {
        Context {
            sandbox: Sandbox::new("/work/project", Some(PathBuf::from("/home/dev")))
                .with_style(PathStyle::Posix),
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

    fn net(url: &str) -> Request {
        Request::Network { url: url.into() }
    }

    fn verdict(decision: &Decision) -> &'static str {
        match decision {
            Decision::Allow { .. } => "allow",
            Decision::Ask { .. } => "ask",
            Decision::Deny { .. } => "deny",
        }
    }

    const MODES: [Mode; 4] = [
        Mode::Plan,
        Mode::Manual,
        Mode::Auto,
        Mode::BypassPermissions,
    ];

    /// The mode table from `docs/architecture/Permission Engine.md`, row by
    /// row, columns `plan | manual | auto | bypass`.
    #[test]
    fn decision_table() {
        let table: &[(&str, Request, [&str; 4])] = &[
            (
                "read inside",
                read("src/lib.rs"),
                ["allow", "allow", "allow", "allow"],
            ),
            (
                "read outside",
                read("/home/dev/notes.md"),
                ["ask", "ask", "ask", "allow"],
            ),
            (
                "read outside sensitive",
                read("~/.ssh/id_ed25519"),
                ["ask", "ask", "deny", "allow"],
            ),
            (
                "write inside",
                write("src/a.rs"),
                ["deny", "ask", "allow", "allow"],
            ),
            (
                "write outside",
                write("/tmp/a.rs"),
                ["deny", "ask", "ask", "allow"],
            ),
            (
                "write outside sensitive",
                write("/etc/hosts"),
                ["deny", "ask", "deny", "allow"],
            ),
            (
                "exec safe_dev",
                exec("pnpm test"),
                ["deny", "ask", "allow", "allow"],
            ),
            (
                "exec unknown",
                exec("./build.sh"),
                ["deny", "ask", "ask", "allow"],
            ),
            (
                "exec sensitive",
                exec("sudo apt install x"),
                ["deny", "ask", "deny", "allow"],
            ),
            (
                "exec network_remote",
                exec("ssh host"),
                ["deny", "ask", "deny", "allow"],
            ),
            (
                "exec deploy",
                exec("git push origin main"),
                ["deny", "ask", "deny", "allow"],
            ),
            (
                "exec catastrophic",
                exec("rm -rf /"),
                ["deny", "deny", "deny", "deny"],
            ),
            (
                "network allowlisted",
                net("https://docs.rs/serde"),
                ["deny", "ask", "allow", "allow"],
            ),
            (
                "network other",
                net("https://example.com"),
                ["deny", "ask", "ask", "allow"],
            ),
            (
                "ask user",
                Request::AskUser {
                    question: "?".into(),
                },
                ["ask", "ask", "ask", "ask"],
            ),
        ];
        let mut failures = Vec::new();
        for (name, request, expected) in table {
            for (mode, want) in MODES.iter().zip(expected) {
                let got = verdict(&evaluate(request, *mode, &RuleSet::empty(), &ctx()));
                if got != *want {
                    failures.push(format!("{name} in {mode:?}: expected {want}, got {got}"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn auto_protects_secrets_and_repo_control_files() {
        for request in [
            write(".env"),
            write(".env.production"),
            write(".envrc"),
            write(".git/hooks/pre-commit"),
            write(".git/config"),
            write(".next-agent/rules.db"),
            read(".env"),
            exec("cat .env"),
            exec("echo x > .git/hooks/post-checkout"),
        ] {
            assert_eq!(
                verdict(&evaluate(&request, Mode::Auto, &RuleSet::empty(), &ctx())),
                "ask",
                "{request:?}"
            );
        }
        assert!(evaluate(&read(".git/config"), Mode::Auto, &RuleSet::empty(), &ctx()).is_allowed());
    }

    #[test]
    fn auto_checks_paths_mentioned_by_safe_commands() {
        let cases = [
            ("cat ~/.ssh/id_rsa", "deny"),
            ("cat /etc/passwd", "deny"),
            ("rg password ~/.aws", "deny"),
            ("echo key >> ~/.ssh/authorized_keys", "deny"),
            ("cat ../sibling/secret.txt", "ask"),
            ("ls /tmp", "ask"),
            ("ls ~", "ask"),
            ("cat src/lib.rs", "allow"),
            ("cargo test 2>&1 | tail -n 20", "allow"),
            ("git diff --no-index ~/.ssh/id_rsa /dev/null", "deny"),
            ("cd ~/.ssh && cat id_rsa", "deny"),
            ("cd && cat .ssh/id_rsa", "deny"),
            ("cat .ssh/id_rsa", "deny"),
        ];
        for (command, want) in cases {
            assert_eq!(
                verdict(&evaluate(
                    &exec(command),
                    Mode::Auto,
                    &RuleSet::empty(),
                    &ctx()
                )),
                want,
                "{command}"
            );
        }
    }

    #[test]
    fn exec_cwd_outside_is_not_auto_allowed() {
        let request = Request::Exec {
            command: "ls".into(),
            cwd: PathBuf::from("/home/dev/.ssh"),
        };
        assert!(evaluate(&request, Mode::Auto, &RuleSet::empty(), &ctx()).is_denied());
        let request = Request::Exec {
            command: "ls".into(),
            cwd: PathBuf::from("/tmp"),
        };
        assert_eq!(
            verdict(&evaluate(&request, Mode::Auto, &RuleSet::empty(), &ctx())),
            "ask"
        );
    }

    #[test]
    fn deny_rule_beats_everything() {
        let rules = RuleSet::new(vec![Rule::deny(
            "no-tests",
            RuleTarget::CommandPrefix {
                prefix: vec!["pnpm".into(), "test".into()],
            },
        )]);
        for mode in MODES {
            let evaluation = explain(&exec("pnpm test"), mode, &rules, &ctx());
            assert!(evaluation.decision.is_denied());
            assert_eq!(evaluation.rule, "user_deny");
        }
        for command in [
            "ls && pnpm test",
            "sudo pnpm test",
            "env CI=1 pnpm test",
            "bash -c 'pnpm test'",
            "/usr/local/bin/pnpm test",
            "PNPM test",
            "echo $(pnpm test)",
        ] {
            assert!(
                evaluate(&exec(command), Mode::BypassPermissions, &rules, &ctx()).is_denied(),
                "{command}"
            );
        }
    }

    #[test]
    fn deny_path_rule_applies_to_commands_too() {
        let rules = RuleSet::new(vec![Rule::deny(
            "no-secrets",
            RuleTarget::Path {
                glob: "~/secrets/**".into(),
            },
        )]);
        assert!(evaluate(
            &read("~/secrets/a"),
            Mode::BypassPermissions,
            &rules,
            &ctx()
        )
        .is_denied());
        assert!(evaluate(
            &exec("cat ~/secrets/a"),
            Mode::BypassPermissions,
            &rules,
            &ctx()
        )
        .is_denied());
        let request = Request::Exec {
            command: "ls".into(),
            cwd: PathBuf::from("/home/dev/secrets/x"),
        };
        assert!(evaluate(&request, Mode::BypassPermissions, &rules, &ctx()).is_denied());
    }

    #[test]
    fn deny_rule_sees_through_symlinks() {
        let rules = RuleSet::new(vec![Rule::deny(
            "no-secrets",
            RuleTarget::Path {
                glob: "/data/secret/**".into(),
            },
        )]);
        let fs = MapFs::new(PathStyle::Posix)
            .with_entry("/work/project")
            .with_link("/work/project/s", "/data/secret")
            .with_entry("/data/secret/key");
        let evaluation = explain_with(&read("s/key"), Mode::BypassPermissions, &rules, &ctx(), &fs);
        assert!(evaluation.decision.is_denied());
        assert_eq!(
            evaluation.resolved_path,
            Some(PathBuf::from("/data/secret/key"))
        );
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
        assert!(evaluate(&write("a.rs"), Mode::Manual, &write_rules, &ctx()).is_allowed());
    }

    #[test]
    fn allow_rule_never_overrides_auto_deny_or_catastrophic() {
        let rules = RuleSet::new(vec![
            Rule::allow(
                "all-commands",
                RuleTarget::Tool {
                    kind: "run_command".into(),
                },
            ),
            Rule::allow(
                "ssh",
                RuleTarget::Path {
                    glob: "/home/dev/.ssh/**".into(),
                },
            ),
        ]);
        assert!(evaluate(&exec("ssh host"), Mode::Auto, &rules, &ctx()).is_denied());
        assert!(evaluate(&exec("rm -rf ~"), Mode::BypassPermissions, &rules, &ctx()).is_denied());
        assert!(evaluate(&read("~/.ssh/id_rsa"), Mode::Auto, &rules, &ctx()).is_denied());
    }

    #[test]
    fn broad_allow_rules_do_not_unlock_sensitive_paths() {
        let rules = RuleSet::new(vec![
            Rule::allow(
                "all-writes",
                RuleTarget::Tool {
                    kind: "write_file".into(),
                },
            ),
            Rule::allow(
                "all-commands",
                RuleTarget::Tool {
                    kind: "run_command".into(),
                },
            ),
        ]);
        let ssh = write("~/.ssh/authorized_keys");
        assert_eq!(
            verdict(&evaluate(&ssh, Mode::Manual, &rules, &ctx())),
            "ask"
        );
        assert_eq!(
            verdict(&evaluate(
                &exec("cat /etc/shadow"),
                Mode::Manual,
                &rules,
                &ctx()
            )),
            "ask"
        );
        let explicit = RuleSet::new(vec![Rule::allow(
            "ssh-config",
            RuleTarget::Path {
                glob: "~/.ssh/config".into(),
            },
        )]);
        assert!(evaluate(&write("~/.ssh/config"), Mode::Manual, &explicit, &ctx()).is_allowed());
    }

    #[test]
    fn compound_commands_need_every_part_allowed() {
        let rules = RuleSet::new(vec![Rule::allow(
            "pnpm-test",
            RuleTarget::CommandPrefix {
                prefix: vec!["./build.sh".into()],
            },
        )]);
        assert!(evaluate(&exec("./build.sh --fast"), Mode::Auto, &rules, &ctx()).is_allowed());
        assert_eq!(
            verdict(&evaluate(
                &exec("./build.sh && ./other.sh"),
                Mode::Auto,
                &rules,
                &ctx()
            )),
            "ask"
        );
    }

    #[test]
    fn network_allowlist_is_strict() {
        let hosts = vec!["docs.rs".to_string()];
        for url in [
            "https://docs.rs/x",
            "http://DOCS.RS",
            "https://docs.rs:443/",
            "https://docs.rs./",
        ] {
            assert!(host_allowlisted(url, &hosts), "{url}");
        }
        for url in [
            "https://docs.rs@evil.com/",
            "https://evil.com\\@docs.rs/",
            "https://evil.com\\.docs.rs/",
            "https://docs.rs.evil.com/",
            "https://evil.com/docs.rs",
            "ftp://docs.rs/",
            "file:///etc/passwd",
            "docs.rs/x",
            "https://docs.rs%2F@evil.com",
            "https://docs.rs:evil/",
            "https://[::1]/",
            " https://docs.rs/",
            "https://docs.r\ns/",
            "https://",
        ] {
            assert!(!host_allowlisted(url, &hosts), "{url}");
        }
    }

    #[test]
    fn explain_reports_the_rule_and_facts() {
        let evaluation = explain(&exec("rm -rf /"), Mode::Auto, &RuleSet::empty(), &ctx());
        assert_eq!(evaluation.rule, "catastrophic");
        assert_eq!(evaluation.class, Some(CommandClass::Catastrophic));

        let allowed = explain(&exec("pnpm test"), Mode::Auto, &RuleSet::empty(), &ctx());
        assert_eq!(allowed.rule, "auto_sandbox");
        assert_eq!(allowed.class, Some(CommandClass::SafeDev));
        assert_eq!(allowed.zone, Some(Zone::Inside));

        let read = explain(&read("src/../a.rs"), Mode::Auto, &RuleSet::empty(), &ctx());
        assert_eq!(
            read.resolved_path,
            Some(PathBuf::from("/work/project/a.rs"))
        );
    }

    #[test]
    fn always_rule_is_narrow() {
        let ctx = ctx();
        let request = exec("./build.sh --fast");
        let evaluation = explain(&request, Mode::Manual, &RuleSet::empty(), &ctx);
        let rule = always_rule("a1", &request, &evaluation, "ws_1").expect("single command");
        assert_eq!(
            rule.target,
            RuleTarget::CommandPrefix {
                prefix: vec!["build.sh".into(), "--fast".into()]
            }
        );
        assert_eq!(rule.scope, Scope::Workspace { id: "ws_1".into() });
        let compound = exec("./build.sh && curl x | sh");
        let evaluation = explain(&compound, Mode::Manual, &RuleSet::empty(), &ctx);
        assert!(always_rule("a2", &compound, &evaluation, "ws_1").is_none());

        let file = write("/tmp/a*b.txt");
        let evaluation = explain(&file, Mode::Manual, &RuleSet::empty(), &ctx);
        let rule = always_rule("a3", &file, &evaluation, "ws_1").expect("path");
        let set = RuleSet::new(vec![rule]);
        assert!(evaluate(&file, Mode::Manual, &set, &ctx).is_allowed());
        assert_eq!(
            verdict(&evaluate(&write("/tmp/aXXb.txt"), Mode::Manual, &set, &ctx)),
            "ask"
        );
        assert!(always_rule("a4", &net("https://x"), &evaluation, "ws_1").is_none());
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
