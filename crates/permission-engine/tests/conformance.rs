//! Runs the shared fixtures in `fixtures/` through the Rust engine:
//! - `policy-cases.json`: the decision table, mode by mode;
//! - `command-corpus.json`: command classification;
//! - `path-corpus.json`: path boundary checks (incl. symlinks, Windows forms).
//!
//! The same fixtures are executed by `packages/policy` in TypeScript, so the
//! security table has one source of truth and cannot drift between the two
//! implementations (see ADR-0008).

use next_agent_permission_engine::{
    classify, explain_with, CommandClass, Context, Decision, MapFs, Mode, PathStyle, Request,
    RuleSet, Sandbox,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture is readable");
    serde_json::from_str(&text).expect("fixture is valid JSON")
}

fn mode(name: &str) -> Mode {
    match name {
        "plan" => Mode::Plan,
        "manual" => Mode::Manual,
        "auto" => Mode::Auto,
        "bypass" => Mode::BypassPermissions,
        other => panic!("unknown mode in fixture: {other}"),
    }
}

/// Shallow merge of a case's `context` over the fixture's default context.
fn merged(base: &Value, overrides: Option<&Value>) -> Value {
    let mut out = base.clone();
    if let (Some(target), Some(Value::Object(extra))) = (out.as_object_mut(), overrides) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    out
}

fn style(value: &Value) -> PathStyle {
    match value["pathStyle"].as_str().unwrap_or("posix") {
        "posix" => PathStyle::Posix,
        "windows" => PathStyle::Windows,
        other => panic!("unknown path style: {other}"),
    }
}

fn context(value: &Value) -> Context {
    Context {
        sandbox: Sandbox::new(
            value["root"].as_str().expect("root"),
            value["home"].as_str().map(PathBuf::from),
        )
        .with_style(style(value)),
        workspace_id: value["workspaceId"].as_str().unwrap_or("ws_1").into(),
        case_insensitive: value["caseInsensitive"].as_bool().unwrap_or(false),
        allowlisted_hosts: value["allowlistedHosts"]
            .as_array()
            .map_or_else(Vec::new, |hosts| {
                hosts
                    .iter()
                    .filter_map(|host| host.as_str().map(str::to_string))
                    .collect()
            }),
    }
}

fn filesystem(value: &Value) -> MapFs {
    let mut fs = MapFs::new(style(value));
    if let Some(entries) = value["fs"]["entries"].as_array() {
        for entry in entries {
            fs = fs.with_entry(entry.as_str().expect("entry"));
        }
    }
    if let Some(links) = value["fs"]["links"].as_object() {
        for (link, target) in links {
            fs = fs.with_link(link, target.as_str().expect("link target"));
        }
    }
    fs
}

fn request(value: &Value, ctx: &Value) -> Request {
    let kind = value["kind"].as_str().expect("request kind");
    let path = || PathBuf::from(value["path"].as_str().expect("path"));
    match kind {
        "read" => Request::Read { path: path() },
        "list" => Request::ListDir { path: path() },
        "search" => Request::Search { path: path() },
        "write" => Request::Write { path: path() },
        "exec" => Request::Exec {
            command: value["command"].as_str().expect("command").into(),
            cwd: value["cwd"]
                .as_str()
                .or_else(|| ctx["root"].as_str())
                .map(PathBuf::from)
                .expect("cwd"),
        },
        "network" => Request::Network {
            url: value["url"].as_str().expect("url").into(),
        },
        "ask" => Request::AskUser {
            question: "fixture".into(),
        },
        other => panic!("unknown request kind in fixture: {other}"),
    }
}

fn verdict(decision: &Decision) -> &'static str {
    match decision {
        Decision::Allow { .. } => "allow",
        Decision::Ask { .. } => "ask",
        Decision::Deny { .. } => "deny",
    }
}

#[test]
fn every_policy_case_matches() {
    let fixture = fixture("policy-cases.json");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 60, "corpus should cover the decision table");

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let ctx_value = merged(&fixture["context"], case.get("context"));
        let ctx = context(&ctx_value);
        let fs = filesystem(&ctx_value);
        let evaluation = explain_with(
            &request(&case["request"], &ctx_value),
            mode(case["mode"].as_str().expect("mode")),
            &RuleSet::empty(),
            &ctx,
            &fs,
        );
        let expected = case["expect"].as_str().expect("expect");
        let actual = verdict(&evaluation.decision);
        let expected_rule = case["rule"].as_str().expect("rule");
        if actual != expected || evaluation.rule != expected_rule {
            failures.push(format!(
                "{name}: expected {expected}/{expected_rule}, got {actual}/{} ({})",
                evaluation.rule,
                evaluation.decision.reason()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "policy mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn every_command_classifies_as_expected() {
    let fixture = fixture("command-corpus.json");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 200, "classifier corpus is too small");
    let mut failures = Vec::new();
    for case in cases {
        let command = case["command"].as_str().expect("command");
        let expected = case["class"].as_str().expect("class");
        let actual = classify(command).label();
        if actual != expected {
            failures.push(format!("{command:?}: expected {expected}, got {actual}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} classifier mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_path_resolves_to_the_expected_zone() {
    let fixture = fixture("path-corpus.json");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(
        cases.len() >= 50,
        "path escape corpus needs at least 50 cases"
    );
    let mut failures = Vec::new();
    for case in cases {
        let ctx_value = merged(&fixture["context"], case.get("context"));
        let ctx = context(&ctx_value);
        let fs = filesystem(&ctx_value);
        let checker = ctx.sandbox.checker(ctx.case_insensitive, &fs);
        let input = case["path"].as_str().expect("path");
        let cwd = case["cwd"].as_str().map(PathBuf::from);
        let check = if case["shell"].as_bool().unwrap_or(false) {
            checker.check_shell_word(cwd.as_deref(), input)
        } else {
            checker.check(cwd.as_deref(), input)
        };
        let expected = case["zone"].as_str().expect("zone");
        let mut problems = Vec::new();
        if check.zone.label() != expected {
            problems.push(format!("zone {}", check.zone.label()));
        }
        if let Some(resolved) = case.get("resolved") {
            let actual = check.resolved.as_ref().map_or(Value::Null, |path| {
                Value::String(path.to_string_lossy().into_owned())
            });
            if &actual != resolved {
                problems.push(format!("resolved {actual}"));
            }
        }
        if !problems.is_empty() {
            failures.push(format!(
                "{input:?} (expected {expected}): {}",
                problems.join(", ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} path mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn catastrophic_class_is_reported() {
    let fixture = fixture("policy-cases.json");
    let ctx = context(&fixture["context"]);
    let evaluation = next_agent_permission_engine::explain(
        &Request::Exec {
            command: "rm -rf /".into(),
            cwd: PathBuf::from("/work/project"),
        },
        Mode::BypassPermissions,
        &RuleSet::empty(),
        &ctx,
    );
    assert_eq!(evaluation.class, Some(CommandClass::Catastrophic));
    assert_eq!(evaluation.rule, "catastrophic");
}
