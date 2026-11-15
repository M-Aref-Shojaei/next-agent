// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/channels/channel-config.ts, src/security/dm-policy-shared.ts @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! Per-channel configuration. Stored as JSON in SQLite (`channels.config`);
//! the bot token is never part of it (it lives in the OS keychain).
//!
//! Stricter than `OpenClaw`: [`ChannelMode`] has no `auto` or bypass variant,
//! so a channel task cannot be configured to run without approvals.

use crate::profile;
use next_agent_permission_engine::Mode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Who may DM the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DmPolicy {
    /// Unknown senders get a pairing code the owner approves on the desktop.
    #[default]
    Pairing,
    /// Only senders on the allow-list; others are ignored.
    Allowlist,
    /// Anyone, but only with an explicit `*` on the allow-list.
    Open,
    /// No DMs at all.
    Disabled,
}

/// Which groups the bot answers in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupPolicy {
    /// Groups are ignored.
    #[default]
    Disabled,
    /// Only the group ids in [`ChannelConfig::groups`].
    Allowlist,
    /// Any group the bot is in (senders are still checked).
    Open,
}

/// How DMs map to sessions (`dmScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DmScope {
    /// Every DM shares one session.
    Main,
    /// One session per person, across channels (with identity links).
    PerPeer,
    /// One session per person per channel. Our default.
    #[default]
    PerChannelPeer,
    /// Also split by bot account.
    PerAccountChannelPeer,
}

/// Permission mode for channel tasks. Deliberately only the two modes that
/// keep a human in the loop: no `auto`, never bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChannelMode {
    /// Reads run; writes and commands wait for approval on the desktop.
    #[default]
    Manual,
    /// Read-only investigation; mutations are denied.
    Plan,
}

impl ChannelMode {
    /// The permission-engine mode a channel task runs in.
    #[must_use]
    pub fn mode(self) -> Mode {
        match self {
            Self::Manual => Mode::Manual,
            Self::Plan => Mode::Plan,
        }
    }

    /// Wire name (`manual` / `plan`), matching the desktop mode names.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Plan => "plan",
        }
    }

    /// Parse a requested mode. Anything but `manual` or `plan` (notably
    /// `auto` and `bypassPermissions`) is refused.
    ///
    /// # Errors
    /// The mode is not allowed for channel tasks.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "manual" => Ok(Self::Manual),
            "plan" => Ok(Self::Plan),
            other => Err(format!(
                "mode `{other}` is not allowed for channel tasks (only manual or plan)"
            )),
        }
    }
}

/// Refuse any mode a channel task must never run in. The desktop calls this
/// right before spawning, whatever the configuration said.
///
/// # Errors
/// `mode` is `auto` or `bypassPermissions`.
pub fn guard_mode(mode: Mode) -> Result<Mode, String> {
    match mode {
        Mode::Manual | Mode::Plan => Ok(mode),
        other => Err(format!(
            "channel tasks never run in {other:?} mode (only manual or plan)"
        )),
    }
}

/// Default seconds a channel task waits for a desktop approval before the
/// answer becomes deny.
pub const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 300;
const MIN_APPROVAL_TIMEOUT_SECS: u64 = 10;
const MAX_APPROVAL_TIMEOUT_SECS: u64 = 3_600;

/// One configured channel account (a Telegram bot, later a Discord bot …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChannelConfig {
    /// Stable id (`ch_…`); also the keychain account suffix for the token.
    pub id: String,
    /// Channel kind: `telegram`.
    pub kind: String,
    /// Label shown in the UI and the timeline.
    pub name: String,
    pub enabled: bool,
    /// Workspace channel tasks run in.
    pub workspace_id: String,
    pub mode: ChannelMode,
    pub dm_policy: DmPolicy,
    pub group_policy: GroupPolicy,
    /// Group chat ids allowed under [`GroupPolicy::Allowlist`].
    pub groups: Vec<String>,
    /// In groups, answer only when mentioned or replied to.
    pub require_mention: bool,
    pub dm_scope: DmScope,
    /// Canonical name → identities (`telegram:123`, or a bare id) that share
    /// one DM session.
    pub identity_links: BTreeMap<String, Vec<String>>,
    pub approval_timeout_secs: u64,
    /// Tools channel tasks may use (deny-by-default; see [`profile`]).
    pub tools: Vec<String>,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: "telegram".into(),
            name: "Telegram".into(),
            enabled: true,
            workspace_id: String::new(),
            mode: ChannelMode::Manual,
            dm_policy: DmPolicy::Pairing,
            group_policy: GroupPolicy::Disabled,
            groups: Vec::new(),
            require_mention: true,
            dm_scope: DmScope::PerChannelPeer,
            identity_links: BTreeMap::new(),
            approval_timeout_secs: DEFAULT_APPROVAL_TIMEOUT_SECS,
            tools: profile::DEFAULT_TOOLS
                .iter()
                .map(|t| (*t).to_string())
                .collect(),
        }
    }
}

impl ChannelConfig {
    /// Normalise and check a configuration before it is saved or used.
    /// `allow_wildcard` says whether the stored allow-list holds `*` (needed
    /// for [`DmPolicy::Open`]).
    ///
    /// # Errors
    /// A human-readable reason the configuration cannot be used.
    pub fn validate(&mut self, allow_wildcard: bool) -> Result<(), String> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = "Telegram".into();
        }
        if self.kind != "telegram" {
            return Err(format!("unknown channel kind `{}`", self.kind));
        }
        if self.workspace_id.trim().is_empty() {
            return Err("pick the workspace channel tasks run in".into());
        }
        if self.dm_policy == DmPolicy::Open && !allow_wildcard {
            return Err("the open DM policy needs an explicit `*` entry on the allow-list".into());
        }
        self.approval_timeout_secs = self
            .approval_timeout_secs
            .clamp(MIN_APPROVAL_TIMEOUT_SECS, MAX_APPROVAL_TIMEOUT_SECS);
        self.groups = clean_ids(&self.groups);
        self.tools = profile::sanitize_tools(&self.tools);
        Ok(())
    }
}

fn clean_ids(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ChannelConfig {
        ChannelConfig {
            id: "ch_1".into(),
            workspace_id: "ws_1".into(),
            ..ChannelConfig::default()
        }
    }

    #[test]
    fn defaults_are_the_strict_ones() {
        let c = ChannelConfig::default();
        assert_eq!(c.dm_policy, DmPolicy::Pairing);
        assert_eq!(c.group_policy, GroupPolicy::Disabled);
        assert_eq!(c.dm_scope, DmScope::PerChannelPeer);
        assert_eq!(c.mode, ChannelMode::Manual);
        assert!(c.require_mention);
    }

    #[test]
    fn open_policy_needs_explicit_wildcard() {
        let mut c = config();
        c.dm_policy = DmPolicy::Open;
        assert!(c.validate(false).unwrap_err().contains('*'));
        assert!(c.validate(true).is_ok());
    }

    #[test]
    fn modes_other_than_manual_and_plan_are_refused() {
        assert_eq!(ChannelMode::parse("manual"), Ok(ChannelMode::Manual));
        assert_eq!(ChannelMode::parse("plan"), Ok(ChannelMode::Plan));
        for bad in ["auto", "bypassPermissions", "bypass", ""] {
            assert!(ChannelMode::parse(bad).is_err(), "{bad}");
        }
        let json = r#"{"id":"ch_1","workspaceId":"w","mode":"bypassPermissions"}"#;
        assert!(serde_json::from_str::<ChannelConfig>(json).is_err());
        let json = r#"{"id":"ch_1","workspaceId":"w","mode":"auto"}"#;
        assert!(serde_json::from_str::<ChannelConfig>(json).is_err());
    }

    #[test]
    fn guard_mode_never_lets_bypass_or_auto_through() {
        assert_eq!(guard_mode(Mode::Manual), Ok(Mode::Manual));
        assert_eq!(guard_mode(Mode::Plan), Ok(Mode::Plan));
        assert!(guard_mode(Mode::Auto).is_err());
        assert!(guard_mode(Mode::BypassPermissions).is_err());
    }

    #[test]
    fn validate_clamps_timeout_and_cleans_lists() {
        let mut c = config();
        c.approval_timeout_secs = 0;
        c.groups = vec![" -100 ".into(), "-100".into(), String::new()];
        c.validate(false).expect("valid");
        assert_eq!(c.approval_timeout_secs, MIN_APPROVAL_TIMEOUT_SECS);
        assert_eq!(c.groups, ["-100"]);
        c.workspace_id = " ".into();
        assert!(c.validate(false).is_err());
    }

    #[test]
    fn round_trips_as_camel_case_json() {
        let c = config();
        let json = serde_json::to_value(&c).expect("json");
        assert_eq!(json["dmPolicy"], "pairing");
        assert_eq!(json["dmScope"], "per-channel-peer");
        let back: ChannelConfig = serde_json::from_value(json).expect("back");
        assert_eq!(back, c);
    }
}
