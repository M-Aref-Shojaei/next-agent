// Design adapted from opencode (MIT, © 2025 opencode): packages/core/src/tool/plugin/subagent.ts, packages/core/src/session/subagent-job.ts @ cbe71595e1f370ff91b3081872b96bbe56519c41
// and Hermes Agent (MIT, © 2025 Nous Research): tools/delegate_tool.py @ a9ee443d58714a73805bd0e4551091543aa479c6
//! The one `subagent` tool, merging opencode's subagent tool (named agent
//! profiles, child session, report on finish) with Hermes' `delegate` (a
//! batch of tasks with `goal` / `context` / `output_schema`, depth and
//! concurrency limits, a compact `{status, exit_reason, truncated,
//! schema_valid}` result).
//!
//! The orchestration lives in [`BuiltinExecutor`](crate::builtin::BuiltinExecutor):
//! children are nested executors whose tool calls are surfaced through the
//! parent's step stream, so every child call goes through the **same**
//! runner, broker and permission engine as the parent's own calls, in the
//! parent's sandbox root, and its events land in the parent task's timeline
//! (call ids prefixed with the subagent id). A child's agent-profile chain
//! includes every ancestor, so it can never do what its parent couldn't.
//!
//! This module holds the pure parts: argument parsing, the child prompt, the
//! report and a small JSON Schema check.

use crate::profiles::AgentProfile;
use serde_json::{json, Value};

/// Tool name.
pub const TOOL: &str = "subagent";

/// Tools a child never inherits (Hermes blocks clarify, memory and cron; the
/// subagent tool itself is removed once the depth limit is reached).
pub const BLOCKLIST: [&str; 4] = ["ask_user", "clarify", "memory", "cron"];

/// Limits for subagents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubagentLimits {
    /// Deepest nesting allowed: with 1, only the task's own agent may spawn.
    pub max_depth: usize,
    /// Most tasks in one `subagent` call (children run interleaved).
    pub max_concurrency: usize,
    /// Model turns for a child whose profile sets no `steps`.
    pub default_steps: usize,
    /// Longest summary returned to the parent, in bytes.
    pub summary_limit: usize,
}

impl Default for SubagentLimits {
    fn default() -> Self {
        Self {
            max_depth: 1,
            max_concurrency: 3,
            default_steps: 25,
            summary_limit: 8 * 1024,
        }
    }
}

/// One task for a child agent.
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentTask {
    pub goal: String,
    pub context: String,
    /// Profile name (default `general`).
    pub agent: String,
    pub output_schema: Option<Value>,
}

/// Parse the tool arguments: `{tasks: [...]}` or a single task inline.
///
/// # Errors
/// A message for missing goals, too many tasks, or a non-object schema.
pub fn parse_tasks(args: &Value, limits: &SubagentLimits) -> Result<Vec<SubagentTask>, String> {
    let items: Vec<&Value> = match args.get("tasks") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(_) => return Err("`tasks` must be an array".into()),
        None => vec![args],
    };
    if items.is_empty() {
        return Err("give at least one task".into());
    }
    if items.len() > limits.max_concurrency {
        return Err(format!(
            "at most {} tasks per call (got {})",
            limits.max_concurrency,
            items.len()
        ));
    }
    items
        .into_iter()
        .map(|item| {
            let text = |key: &str| {
                item.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let goal = text("goal");
            if goal.is_empty() {
                return Err("every task needs a non-empty `goal`".to_string());
            }
            let agent = match text("agent") {
                agent if agent.is_empty() => "general".to_string(),
                agent => agent,
            };
            let output_schema = match item.get("output_schema") {
                None | Some(Value::Null) => None,
                Some(schema @ Value::Object(_)) => Some(schema.clone()),
                Some(_) => return Err("`output_schema` must be a JSON Schema object".into()),
            };
            Ok(SubagentTask {
                goal,
                context: text("context"),
                agent,
                output_schema,
            })
        })
        .collect()
}

/// The child's first (and only) user message.
#[must_use]
pub fn child_prompt(task: &SubagentTask) -> String {
    let mut prompt = task.goal.clone();
    if !task.context.is_empty() {
        prompt.push_str("\n\nContext from the parent agent:\n");
        prompt.push_str(&task.context);
    }
    prompt.push_str(
        "\n\nYou start with no other context: the parent sees only your final message. When you are done, reply without tool calls; that reply is your report.",
    );
    if let Some(schema) = &task.output_schema {
        prompt.push_str(
            "\nYour final message must be a single JSON value (optionally in a ```json fence) matching this JSON Schema:\n```json\n",
        );
        prompt.push_str(&serde_json::to_string_pretty(schema).unwrap_or_default());
        prompt.push_str("\n```");
    }
    prompt
}

/// What the parent receives for one child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub id: String,
    pub agent: String,
    /// `completed` or `failed`.
    pub status: &'static str,
    /// `done`, `max_steps` or `error`.
    pub exit_reason: &'static str,
    pub truncated: bool,
    /// `None` when no `output_schema` was given.
    pub schema_valid: Option<bool>,
    pub summary: String,
    pub steps: usize,
}

impl Report {
    /// Build a report, truncating the summary and checking the schema.
    #[must_use]
    pub fn new(
        id: &str,
        agent: &str,
        exit_reason: &'static str,
        text: &str,
        schema: Option<&Value>,
        limits: &SubagentLimits,
        steps: usize,
    ) -> Self {
        let status = if exit_reason == "error" {
            "failed"
        } else {
            "completed"
        };
        let schema_valid = schema
            .map(|schema| extract_json(text).is_some_and(|value| validate(&value, schema).is_ok()));
        let truncated = text.len() > limits.summary_limit;
        Self {
            id: id.to_string(),
            agent: agent.to_string(),
            status,
            exit_reason,
            truncated,
            schema_valid,
            summary: crate::runner::truncate(text, limits.summary_limit),
            steps,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "agent": self.agent,
            "status": self.status,
            "exit_reason": self.exit_reason,
            "truncated": self.truncated,
            "schema_valid": self.schema_valid,
            "summary": self.summary,
        })
    }
}

/// The tool result for a finished `subagent` call: one report object, or
/// `{"results": [...]}` for several.
#[must_use]
pub fn result_json(reports: &[Report]) -> String {
    let value = match reports {
        [one] => one.to_json(),
        many => json!({ "results": many.iter().map(Report::to_json).collect::<Vec<_>>() }),
    };
    value.to_string()
}

/// `OpenAI` function schema for the tool, listing the spawnable profiles.
#[must_use]
pub fn tool_schema(profiles: &[AgentProfile], limits: &SubagentLimits) -> Value {
    let agents: Vec<&AgentProfile> = profiles.iter().filter(|p| p.mode.subagent()).collect();
    let names: Vec<&str> = agents.iter().map(|p| p.name.as_str()).collect();
    let listing = agents
        .iter()
        .map(|p| format!("- {}: {}", p.name, p.description))
        .collect::<Vec<_>>()
        .join("\n");
    let task = json!({
        "type": "object",
        "properties": {
            "goal": {"type": "string", "description": "What the subagent must achieve"},
            "context": {"type": "string", "description": "Everything it needs to know: it starts with no other context"},
            "agent": {"type": "string", "enum": names, "description": "Agent profile (default general)"},
            "output_schema": {"type": "object", "description": "Optional JSON Schema the final report must match"}
        },
        "required": ["goal"],
        "additionalProperties": false
    });
    json!({
        "type": "function",
        "function": {
            "name": TOOL,
            "description": format!(
                "Delegate up to {} independent tasks to subagents that run with your tools and permissions (never more) in the same workspace, then return their reports as {{status, exit_reason, truncated, schema_valid, summary}}. Available agents:\n{listing}",
                limits.max_concurrency
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "tasks": {"type": "array", "items": task, "minItems": 1, "maxItems": limits.max_concurrency}
                },
                "required": ["tasks"],
                "additionalProperties": false
            }
        }
    })
}

/// Find a JSON value in a report: the whole text, a fenced `json` block, or the
/// outermost `{…}` / `[…]` span.
#[must_use]
pub fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
    }
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        if let Some(end) = after.find("```") {
            if let Ok(value) = serde_json::from_str(after[..end].trim()) {
                return Some(value);
            }
        }
    }
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(start), Some(end)) = (trimmed.find(open), trimmed.rfind(close)) {
            if start < end {
                if let Ok(value) = serde_json::from_str(&trimmed[start..=end]) {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Check `value` against the commonly used JSON Schema subset: `type`,
/// `properties`, `required`, `items`, `enum`, `additionalProperties: false`.
///
/// # Errors
/// The first mismatch, as a JSON-pointer-ish path and reason.
pub fn validate(value: &Value, schema: &Value) -> Result<(), String> {
    validate_at(value, schema, "$")
}

fn validate_at(value: &Value, schema: &Value, at: &str) -> Result<(), String> {
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            return Err(format!("{at}: not one of the allowed values"));
        }
    }
    if let Some(kind) = schema.get("type") {
        let kinds: Vec<&str> = match kind {
            Value::String(one) => vec![one.as_str()],
            Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !kinds.is_empty() && !kinds.iter().any(|kind| matches_type(value, kind)) {
            return Err(format!("{at}: expected {}", kinds.join(" or ")));
        }
    }
    if let Value::Object(map) = value {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(key) {
                    return Err(format!("{at}: missing required `{key}`"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, item) in map {
            match properties.and_then(|p| p.get(key)) {
                Some(sub) => validate_at(item, sub, &format!("{at}.{key}"))?,
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return Err(format!("{at}: unexpected property `{key}`"));
                }
                None => {}
            }
        }
    }
    if let (Value::Array(items), Some(sub)) = (value, schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            validate_at(item, sub, &format!("{at}[{index}]"))?;
        }
    }
    Ok(())
}

fn matches_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_batches_and_single_tasks() {
        let limits = SubagentLimits::default();
        let batch = parse_tasks(
            &json!({"tasks": [{"goal": "a", "agent": "explore"}, {"goal": "b", "context": "c"}]}),
            &limits,
        )
        .expect("batch");
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].agent, "explore");
        assert_eq!(batch[1].agent, "general");
        assert_eq!(batch[1].context, "c");
        let single = parse_tasks(&json!({"goal": "x"}), &limits).expect("single");
        assert_eq!(single.len(), 1);
        assert!(parse_tasks(&json!({"tasks": []}), &limits).is_err());
        assert!(parse_tasks(&json!({"tasks": [{"context": "no goal"}]}), &limits).is_err());
        let too_many: Vec<Value> = (0..4).map(|i| json!({"goal": i.to_string()})).collect();
        assert!(parse_tasks(&json!({ "tasks": too_many }), &limits)
            .unwrap_err()
            .contains("at most 3"));
        assert!(parse_tasks(&json!({"goal": "x", "output_schema": "nope"}), &limits).is_err());
    }

    #[test]
    fn child_prompt_carries_goal_context_and_schema() {
        let prompt = child_prompt(&SubagentTask {
            goal: "Find the bug".into(),
            context: "It is in src/".into(),
            agent: "explore".into(),
            output_schema: Some(json!({"type": "object"})),
        });
        assert!(prompt.starts_with("Find the bug"));
        assert!(prompt.contains("It is in src/"));
        assert!(prompt.contains("JSON Schema"));
    }

    #[test]
    fn reports_truncate_and_check_schemas() {
        let limits = SubagentLimits {
            summary_limit: 10,
            ..SubagentLimits::default()
        };
        let schema = json!({
            "type": "object",
            "required": ["files"],
            "properties": {"files": {"type": "array", "items": {"type": "string"}}},
            "additionalProperties": false
        });
        let ok = Report::new(
            "s1",
            "explore",
            "done",
            "```json\n{\"files\":[\"a\"]}\n```",
            Some(&schema),
            &SubagentLimits::default(),
            2,
        );
        assert_eq!(ok.schema_valid, Some(true));
        assert_eq!(ok.status, "completed");
        let bad = Report::new(
            "s1",
            "explore",
            "done",
            "{\"files\":[1]}",
            Some(&schema),
            &limits,
            2,
        );
        assert_eq!(bad.schema_valid, Some(false));
        assert!(bad.truncated);
        let failed = Report::new("s2", "general", "error", "boom", None, &limits, 1);
        assert_eq!((failed.status, failed.schema_valid), ("failed", None));
        let one: Value =
            serde_json::from_str(&result_json(std::slice::from_ref(&failed))).expect("json");
        assert_eq!(one["status"], "failed");
        assert!(one.get("results").is_none());
        let many: Value = serde_json::from_str(&result_json(&[ok, failed])).expect("json");
        assert_eq!(many["results"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn schema_subset() {
        let schema = json!({"type": "object", "properties": {"n": {"type": "integer"}, "k": {"enum": ["a", "b"]}}, "required": ["n"]});
        assert!(validate(&json!({"n": 1, "k": "a"}), &schema).is_ok());
        assert!(validate(&json!({"n": 1.5}), &schema).is_err());
        assert!(validate(&json!({"k": "a"}), &schema).is_err());
        assert!(validate(&json!({"n": 1, "k": "z"}), &schema).is_err());
        assert!(validate(&json!([1]), &json!({"type": ["array", "null"]})).is_ok());
        assert_eq!(
            extract_json("Result: {\"a\": 1} done"),
            Some(json!({"a": 1}))
        );
        assert_eq!(extract_json("no json"), None);
    }

    #[test]
    fn schema_lists_only_spawnable_profiles() {
        let schema = tool_schema(
            &crate::profiles::builtin_profiles(),
            &SubagentLimits::default(),
        );
        let names = &schema["function"]["parameters"]["properties"]["tasks"]["items"]["properties"]
            ["agent"]["enum"];
        assert_eq!(names, &json!(["general", "explore"]));
    }
}
