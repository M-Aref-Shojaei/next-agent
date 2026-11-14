// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/security/dangerous-tools.ts @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! The tool profile every channel task runs under.
//!
//! Deny-by-default: `*` is denied and only the configured tools are allowed.
//! The profile sits *above* the permission engine and the task's own agent
//! profile, so it can only tighten: an allowed tool still goes through the
//! engine (in manual mode writes wait for a desktop approval), a denied tool
//! never runs. Tools that control the gateway, schedule jobs, spawn sessions
//! or message other sessions are denied even if a configuration lists them.

use next_agent_core::profiles::{self, AgentProfile};
use next_agent_core::runner::Floor;
use next_agent_core::{AgentExecutor, AgentStep, Emitter, TaskSpec, ToolRequest, ToolResult};
use std::fmt::Write as _;

/// Name of the synthetic profile in the chain (shows in denial reasons).
pub const PROFILE_NAME: &str = "channel";

/// Tools a channel task may use unless configured otherwise: read and edit
/// files in the workspace. Commands, HTTP and questions are opt-in.
pub const DEFAULT_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search",
    "write_file",
    "apply_patch",
    "edit",
];

/// Every tool a channel configuration may enable.
pub const SELECTABLE_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search",
    "write_file",
    "apply_patch",
    "edit",
    "run_command",
    "http_fetch",
];

/// Never available to channel tasks, whatever the configuration says:
/// spawning sessions (`subagent`), cron (`schedule_job`, `list_jobs`,
/// `cancel_job`), sending to other sessions or channels, gateway control.
/// Names follow ours and `OpenClaw`'s `DEFAULT_GATEWAY_HTTP_TOOL_DENY`
/// control-plane entries, so tools that land later are covered from day one.
pub const ALWAYS_DENIED: &[&str] = &[
    "subagent",
    "sessions_spawn",
    "schedule_job",
    "list_jobs",
    "cancel_job",
    "cron",
    "automations",
    "sessions_send",
    "session_send",
    "conversations_send",
    "deliver_to_channel",
    "channel_send",
    "gateway",
    "plugins",
    "ask_user",
];

/// Keep only selectable tools, in a stable order. Unknown and always-denied
/// names are dropped.
#[must_use]
pub fn sanitize_tools(tools: &[String]) -> Vec<String> {
    SELECTABLE_TOOLS
        .iter()
        .filter(|tool| tools.iter().any(|t| t.trim() == **tool))
        .map(|tool| (*tool).to_string())
        .collect()
}

/// The `channel` agent profile: `*: deny`, the configured tools `allow`,
/// [`ALWAYS_DENIED`] `deny` (written last, so it wins).
#[must_use]
pub fn channel_profile(tools: &[String]) -> AgentProfile {
    let mut text = String::from("---\npermissions:\n  \"*\": deny\n");
    for tool in sanitize_tools(tools) {
        let _ = writeln!(text, "  {tool}: allow");
    }
    for tool in ALWAYS_DENIED {
        let _ = writeln!(text, "  {tool}: deny");
    }
    text.push_str(
        "---\nThis task came from a chat channel. Approvals happen only on the desktop.\n",
    );
    let mut profile = profiles::parse_profile(PROFILE_NAME, &text)
        .unwrap_or_else(|error| unreachable!("static channel profile parses: {error}"));
    profile.builtin = true;
    profile
}

/// Wraps any executor so every call carries the channel profile's floor,
/// whichever executor runs (the built-in agent, the scripted demo agent, or
/// an upstream engine). The floor is combined with the executor's own, the
/// stricter wins.
pub struct ChannelExecutor {
    inner: Box<dyn AgentExecutor + Send>,
    chain: Vec<AgentProfile>,
}

impl ChannelExecutor {
    #[must_use]
    pub fn new(inner: Box<dyn AgentExecutor + Send>, profile: AgentProfile) -> Self {
        Self {
            inner,
            chain: vec![profile],
        }
    }
}

impl AgentExecutor for ChannelExecutor {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn attach(&mut self, events: &Emitter) {
        self.inner.attach(events);
    }

    fn start(&mut self, spec: &TaskSpec) -> AgentStep {
        self.inner.start(spec)
    }

    fn floor(&self, call_id: &str, request: &ToolRequest) -> Option<Floor> {
        Floor::strictest(
            profiles::floor_for(&self.chain, request),
            self.inner.floor(call_id, request),
        )
    }

    fn next(&mut self, result: Option<&ToolResult>) -> AgentStep {
        self.inner.next(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_agent_core::profiles::Permission;
    use next_agent_core::MockExecutor;
    use std::path::PathBuf;

    fn read() -> ToolRequest {
        ToolRequest::Read {
            path: PathBuf::from("/w/a"),
        }
    }
    fn exec() -> ToolRequest {
        ToolRequest::Exec {
            command: "ls".into(),
            cwd: PathBuf::from("/w"),
        }
    }

    #[test]
    fn profile_denies_by_default() {
        let profile = channel_profile(&[]);
        for tool in SELECTABLE_TOOLS {
            assert_eq!(
                profile.permission(tool, profiles::policy_kind(tool)),
                Permission::Deny,
                "{tool}"
            );
        }
        assert_eq!(
            profile.permission("brand_new_tool", "brand_new_tool"),
            Permission::Deny
        );
    }

    #[test]
    fn configured_tools_are_allowed_but_always_denied_ones_never() {
        let mut tools: Vec<String> = ALWAYS_DENIED.iter().map(|t| (*t).to_string()).collect();
        tools.push("read_file".into());
        let profile = channel_profile(&tools);
        assert_eq!(
            profile.permission("read_file", "read_file"),
            Permission::Allow
        );
        for tool in ALWAYS_DENIED {
            assert_eq!(profile.permission(tool, tool), Permission::Deny, "{tool}");
        }
        assert_eq!(sanitize_tools(&tools), ["read_file"]);
    }

    #[test]
    fn executor_floor_denies_unlisted_tools_and_passes_listed_ones_to_the_engine() {
        let wrapped = ChannelExecutor::new(
            Box::new(MockExecutor::new(vec![])),
            channel_profile(&["read_file".into()]),
        );
        assert_eq!(wrapped.floor("c1", &read()), None);
        assert!(matches!(wrapped.floor("c2", &exec()), Some(Floor::Deny(_))));
    }

    #[test]
    fn floor_only_tightens_the_inner_executor() {
        struct Asks;
        impl AgentExecutor for Asks {
            fn id(&self) -> &'static str {
                "asks"
            }
            fn start(&mut self, _spec: &TaskSpec) -> AgentStep {
                AgentStep::Done {
                    summary: String::new(),
                }
            }
            fn floor(&self, _call_id: &str, _request: &ToolRequest) -> Option<Floor> {
                Some(Floor::Ask("inner asks".into()))
            }
            fn next(&mut self, _result: Option<&ToolResult>) -> AgentStep {
                unreachable!()
            }
        }
        let wrapped = ChannelExecutor::new(Box::new(Asks), channel_profile(&["read_file".into()]));
        // Channel allows read_file, the inner profile asks: ask wins.
        assert!(matches!(wrapped.floor("c", &read()), Some(Floor::Ask(_))));
        // Channel denies run_command, the inner asks: deny wins.
        assert!(matches!(wrapped.floor("c", &exec()), Some(Floor::Deny(_))));
    }

    #[test]
    fn builtin_profile_chain_drops_denied_tools_from_the_offer() {
        let profile = channel_profile(&["read_file".into()]);
        assert!(profiles::chain_offers(
            std::slice::from_ref(&profile),
            "read_file",
            "read_file"
        ));
        assert!(!profiles::chain_offers(
            std::slice::from_ref(&profile),
            "subagent",
            "subagent"
        ));
        assert!(!profiles::chain_offers(
            &[profile],
            "run_command",
            "run_command"
        ));
    }
}
