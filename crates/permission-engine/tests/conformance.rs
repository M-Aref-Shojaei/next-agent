//! Runs `fixtures/policy-cases.json` through the Rust engine.
//!
//! The same fixture is executed by `packages/policy` in TypeScript, so the
//! security table has one source of truth and cannot drift between the two
//! implementations (see ADR-0008).

use next_agent_permission_engine::{
    CommandClass, Context, Decision, Mode, Request, RuleSet, Sandbox,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/policy-cases.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
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

fn context(value: &Value) -> Context {
    Context {
        sandbox: Sandbox::new(
            value["root"].as_str().expect("root"),
            Some(PathBuf::from(value["home"].as_str().expect("home"))),
        ),
        workspace_id: value["workspaceId"].as_str().expect("workspace id").into(),
        case_insensitive: value["caseInsensitive"].as_bool().expect("flag"),
        allowlisted_hosts: value["allowlistedHosts"]
            .as_array()
            .expect("hosts")
            .iter()
            .filter_map(|host| host.as_str().map(str::to_string))
            .collect(),
    }
}

fn request(value: &Value) -> Request {
    let kind = value["kind"].as_str().expect("request kind");
    match kind {
        "read" => Request::Read {
            path: PathBuf::from(value["path"].as_str().expect("path")),
        },
        "list" => Request::ListDir {
            path: PathBuf::from(value["path"].as_str().expect("path")),
        },
        "search" => Request::Search {
            path: PathBuf::from(value["path"].as_str().expect("path")),
        },
        "write" => Request::Write {
            path: PathBuf::from(value["path"].as_str().expect("path")),
        },
        "exec" => Request::Exec {
            command: value["command"].as_str().expect("command").into(),
            cwd: value["cwd"]
                .as_str()
                .map_or_else(|| PathBuf::from("/work/project"), PathBuf::from),
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

#[test]
fn every_fixture_case_matches() {
    let fixture = fixture();
    let ctx = context(&fixture["context"]);
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 25, "corpus should cover the decision table");

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let evaluation = next_agent_permission_engine::explain(
            &request(&case["request"]),
            mode(case["mode"].as_str().expect("mode")),
            &RuleSet::empty(),
            &ctx,
        );
        let expected = case["expect"].as_str().expect("expect");
        let actual = match &evaluation.decision {
            Decision::Allow { .. } => "allow",
            Decision::Ask { .. } => "ask",
            Decision::Deny { .. } => "deny",
        };
        let expected_rule = case["rule"].as_str().expect("rule");
        if actual != expected || evaluation.rule != expected_rule {
            failures.push(format!(
                "{name}: expected {expected}/{expected_rule}, got {actual}/{}",
                evaluation.rule
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "fixture mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn catastrophic_class_is_reported() {
    let fixture = fixture();
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
