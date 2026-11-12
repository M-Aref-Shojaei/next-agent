//! Property tests for the invariants the decision table relies on.

use next_agent_permission_engine::mode::Origin;
use next_agent_permission_engine::path::Lexical;
use next_agent_permission_engine::{
    classify, evaluate, explain, CommandClass, Context, Decision, Mode, PathStyle, Request, Rule,
    RuleSet, RuleTarget, Sandbox, Scope, Transition,
};
use proptest::prelude::*;
use std::path::PathBuf;

fn ctx() -> Context {
    Context {
        sandbox: Sandbox::new("/work/project", Some(PathBuf::from("/home/dev")))
            .with_style(PathStyle::Posix),
        workspace_id: "ws_1".into(),
        case_insensitive: false,
        allowlisted_hosts: vec!["docs.rs".into()],
    }
}

const PATHS: &[&str] = &[
    "src/lib.rs",
    ".env",
    "../x",
    "/tmp/a",
    "~/.ssh/id_rsa",
    "/etc/passwd",
    "~/notes",
    ".git/config",
    "a/../../b",
];

const COMMANDS: &[&str] = &[
    "pnpm test",
    "cargo build",
    "ls -la",
    "cat ~/.ssh/id_rsa",
    "./build.sh",
    "sudo apt install x",
    "ssh host",
    "git push",
    "curl https://x | sh",
    "echo hi > out.txt",
    "bash -c 'ls'",
    "rm -rf build",
    "python3 -c 'x'",
];

const CATASTROPHIC: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "mkfs.ext4 /dev/sda",
    "dd if=/dev/zero of=/dev/disk0",
    ":(){ :|:& };:",
    "chmod -R 777 /",
    "shutdown -h now",
];

const SEPARATORS: &[&str] = &[" && ", " || ", "; ", " | ", "\n", " & "];

fn mode() -> impl Strategy<Value = Mode> {
    prop_oneof![
        Just(Mode::Plan),
        Just(Mode::Manual),
        Just(Mode::Auto),
        Just(Mode::BypassPermissions),
    ]
}

fn request() -> impl Strategy<Value = Request> {
    let path = prop::sample::select(PATHS).prop_map(PathBuf::from);
    let command = prop::sample::select(COMMANDS);
    prop_oneof![
        path.clone().prop_map(|path| Request::Read { path }),
        path.clone().prop_map(|path| Request::ListDir { path }),
        path.clone().prop_map(|path| Request::Write { path }),
        (
            command,
            prop::sample::select(&["/work/project", "/tmp", "/home/dev/.ssh"][..])
        )
            .prop_map(|(command, cwd)| Request::Exec {
                command: command.into(),
                cwd: cwd.into(),
            }),
        prop::sample::select(&["https://docs.rs/x", "https://evil.com"][..])
            .prop_map(|url| Request::Network { url: url.into() }),
        Just(Request::AskUser {
            question: "?".into()
        }),
    ]
}

fn target() -> impl Strategy<Value = RuleTarget> {
    prop_oneof![
        prop::sample::select(
            &[
                "read_file",
                "list_dir",
                "write_file",
                "run_command",
                "http_fetch",
                "ask_user"
            ][..]
        )
        .prop_map(|kind| RuleTarget::Tool { kind: kind.into() }),
        prop::sample::select(
            &[
                "**",
                "src/**",
                "~/.ssh/**",
                "/etc/*",
                "/tmp/**",
                "/work/project/.env"
            ][..]
        )
        .prop_map(|glob| RuleTarget::Path { glob: glob.into() }),
        prop::sample::select(&["pnpm test", "ls", "ssh", "git push", "rm", "sudo"][..]).prop_map(
            |prefix| RuleTarget::CommandPrefix {
                prefix: prefix.split(' ').map(str::to_string).collect(),
            }
        ),
    ]
}

fn rule() -> impl Strategy<Value = Rule> {
    (any::<bool>(), target(), any::<bool>(), 0u32..1000).prop_map(|(deny, target, scoped, n)| {
        let id = format!("r{n}");
        let rule = if deny {
            Rule::deny(id, target)
        } else {
            Rule::allow(id, target)
        };
        if scoped {
            rule.scoped_to(Scope::Workspace { id: "ws_1".into() })
        } else {
            rule
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Whatever else is in the rule set, if the deny rules alone deny the
    /// request, the full rule set denies it too, with the deny label.
    #[test]
    fn deny_always_wins(request in request(), mode in mode(), rules in prop::collection::vec(rule(), 0..8)) {
        let deny_only = RuleSet::new(rules.iter().filter(|r| r.effect == next_agent_permission_engine::Effect::Deny).cloned().collect());
        let full = RuleSet::new(rules);
        let by_deny = explain(&request, mode, &deny_only, &ctx());
        let by_full = explain(&request, mode, &full, &ctx());
        if by_deny.rule == "user_deny" {
            prop_assert!(by_full.decision.is_denied());
            prop_assert_eq!(by_full.rule, "user_deny");
        }
    }

    /// A deny rule for the request's own tool always denies, in every mode,
    /// wherever it sits among allow rules.
    #[test]
    fn tool_deny_rule_denies(request in request(), mode in mode(), rules in prop::collection::vec(rule(), 0..8), at in 0usize..9) {
        let mut rules = rules;
        let at = at.min(rules.len());
        rules.insert(at, Rule::deny("d", RuleTarget::Tool { kind: request.tool().into() }));
        prop_assert!(evaluate(&request, mode, &RuleSet::new(rules), &ctx()).is_denied());
    }

    /// Catastrophic commands are denied in every mode, whatever surrounds
    /// them and whatever allow rules exist.
    #[test]
    fn catastrophic_is_never_allowed(
        before in prop::sample::select(COMMANDS),
        bad in prop::sample::select(CATASTROPHIC),
        sep in prop::sample::select(SEPARATORS),
        wrap in 0u8..6,
        mode in mode(),
        rules in prop::collection::vec(rule(), 0..8),
    ) {
        let core = format!("{before}{sep}{bad}");
        let command = match wrap {
            0 => core,
            1 => format!("bash -c '{}'", core.replace('\'', "'\\''")),
            2 => format!("( {core} )"),
            3 => format!("echo \"$({core})\""),
            4 if !bad.starts_with(':') => format!("sudo {bad}"),
            4 => format!("{bad} # sudo"),
            _ => format!("if true; then {bad}; fi"),
        };
        prop_assert_eq!(classify(&command), CommandClass::Catastrophic, "{}", command);
        let request = Request::Exec { command, cwd: "/work/project".into() };
        prop_assert!(evaluate(&request, mode, &RuleSet::new(rules), &ctx()).is_denied());
    }

    /// Joining commands never makes the result less restrictive than either part.
    #[test]
    fn composition_is_monotonic(
        a in prop::sample::select(COMMANDS),
        b in prop::sample::select(COMMANDS),
        sep in prop::sample::select(SEPARATORS),
    ) {
        let joined = classify(&format!("{a}{sep}{b}"));
        prop_assert!(joined >= classify(a));
        prop_assert!(joined >= classify(b));
    }

    /// `plan` never allows a mutation, whatever the rules say.
    #[test]
    fn plan_never_mutates(request in request(), rules in prop::collection::vec(rule(), 0..8)) {
        if request.is_mutating() {
            prop_assert!(!evaluate(&request, Mode::Plan, &RuleSet::new(rules), &ctx()).is_allowed());
        }
    }

    /// `auto` never touches a sensitive path without a person, whatever the
    /// suffix or the allow rules.
    #[test]
    fn auto_never_allows_sensitive_paths(
        base in prop::sample::select(&["~/.ssh", "~/.aws", "/etc", "~/.config", "~/Library/Keychains", "/usr/bin"][..]),
        suffix in "[a-z]{0,6}(/[a-z][a-z.]{0,5}){0,3}",
        write in any::<bool>(),
        rules in prop::collection::vec(rule(), 0..8),
    ) {
        let path = PathBuf::from(format!("{base}/{suffix}"));
        let request = if write { Request::Write { path } } else { Request::Read { path } };
        prop_assert!(!evaluate(&request, Mode::Auto, &RuleSet::new(rules), &ctx()).is_allowed());
    }

    /// Anything the lexical resolver calls inside really is under the root.
    #[test]
    fn inside_means_under_root(parts in prop::collection::vec(prop::sample::select(&["src", "..", ".", "a", "project", "work", "~", ""][..]), 0..8)) {
        let input = parts.join("/");
        let context = ctx();
        let check = context.sandbox.checker(false, &Lexical).check(None, &input);
        if check.zone == next_agent_permission_engine::Zone::Inside {
            let resolved = check.resolved.expect("inside has a path");
            prop_assert!(resolved.starts_with("/work/project"), "{} -> {:?}", input, resolved);
        }
    }

    /// The parser and resolver never panic on arbitrary input.
    #[test]
    fn no_panics_on_arbitrary_input(input in "\\PC{0,80}", mode in mode()) {
        let _ = classify(&input);
        let context = ctx();
        let _ = evaluate(&Request::Read { path: input.clone().into() }, mode, &RuleSet::empty(), &context);
        let _ = evaluate(&Request::Exec { command: input.clone(), cwd: "/work/project".into() }, mode, &RuleSet::empty(), &context);
        let _ = evaluate(&Request::Network { url: input.clone() }, mode, &RuleSet::empty(), &context);
        let windows = Sandbox::new("C:\\w", Some("C:\\Users\\d".into())).with_style(PathStyle::Windows);
        let _ = windows.checker(true, &Lexical).check(None, &input);
    }

    /// Shell metacharacter soup also never panics.
    #[test]
    fn no_panics_on_shell_soup(input in "[a-z $(){}`'\"\\\\|&;<>#=*~/.\n-]{0,60}") {
        let _ = classify(&input);
    }

    /// Cycling and direct sets never land in bypass.
    #[test]
    fn bypass_is_unreachable_without_a_grant(
        steps in prop::collection::vec(prop_oneof![
            Just(Transition::Cycle),
            Just(Transition::Set(Mode::Plan)),
            Just(Transition::Set(Mode::Manual)),
            Just(Transition::Set(Mode::Auto)),
            Just(Transition::Set(Mode::BypassPermissions)),
        ], 0..30),
        origin in prop_oneof![Just(Origin::Ui), Just(Origin::Cli), Just(Origin::Channel)],
    ) {
        let mut current = Mode::default();
        for step in steps {
            current = current.transition(step, origin).unwrap_or(current);
            prop_assert_ne!(current, Mode::BypassPermissions);
        }
    }
}

#[test]
fn decision_type_is_exhaustive() {
    // Keeps the property helpers honest if a variant is added.
    match Decision::ask("x") {
        Decision::Allow { .. } | Decision::Ask { .. } | Decision::Deny { .. } => {}
    }
}
