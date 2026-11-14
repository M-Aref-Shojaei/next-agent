// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/channels/direct-dm-access.ts, src/channels/mention-gating.ts, src/channels/allowlist-match.ts @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! Who gets through: DM policy and group gating. Pure functions.
//!
//! Group checks run in this order: group policy → sender allow-list →
//! mention or reply activation. Replying to one of the bot's messages only
//! *activates* the bot in a group; it never authorises the sender (stricter
//! than `OpenClaw`, where a reply can bypass the mention requirement only).

use crate::config::{ChannelConfig, DmPolicy, GroupPolicy};

/// Allow-list entry that admits everyone (needed for [`DmPolicy::Open`]).
pub const WILDCARD: &str = "*";

/// Whether `sender` is on `allow` (exact id match, or `*`).
#[must_use]
pub fn sender_allowed(allow: &[String], sender: &str) -> bool {
    let sender = sender.trim();
    !sender.is_empty()
        && allow
            .iter()
            .any(|entry| entry == WILDCARD || entry == sender)
}

/// Whether `sender` is listed by id (no wildcard).
fn listed(allow: &[String], sender: &str) -> bool {
    let sender = sender.trim();
    !sender.is_empty() && allow.iter().any(|entry| entry == sender)
}

/// Outcome for a direct message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmAccess {
    Allow,
    /// Unknown sender under the pairing policy: issue or repeat a code.
    NeedsPairing,
    /// Ignore silently.
    Deny(&'static str),
}

/// Decide a DM. `allow` is the stored allow-list (paired + manual entries).
#[must_use]
pub fn dm_access(policy: DmPolicy, allow: &[String], sender: &str) -> DmAccess {
    let listed = listed(allow, sender);
    match policy {
        DmPolicy::Disabled => DmAccess::Deny("direct messages are disabled"),
        DmPolicy::Open if allow.iter().any(|e| e == WILDCARD) => DmAccess::Allow,
        DmPolicy::Open => DmAccess::Deny("open policy without an explicit `*`"),
        DmPolicy::Allowlist | DmPolicy::Pairing if listed => DmAccess::Allow,
        DmPolicy::Allowlist => DmAccess::Deny("sender is not on the allow-list"),
        DmPolicy::Pairing => DmAccess::NeedsPairing,
    }
}

/// Facts about a group message the channel adapter extracted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFacts {
    pub group_id: String,
    pub sender_id: String,
    /// The bot was @-mentioned (or addressed with `/command@bot`).
    pub mentioned: bool,
    /// The message replies to one of the bot's messages.
    pub reply_to_bot: bool,
}

/// Outcome for a group message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupAccess {
    Allow,
    /// Ignore silently, with the step that stopped it.
    Skip(&'static str),
}

/// Gate a group message: policy → sender allow-list → activation.
#[must_use]
pub fn group_access(config: &ChannelConfig, allow: &[String], facts: &GroupFacts) -> GroupAccess {
    match config.group_policy {
        GroupPolicy::Disabled => return GroupAccess::Skip("groups are disabled"),
        GroupPolicy::Allowlist
            if !config
                .groups
                .iter()
                .any(|g| g == WILDCARD || g == facts.group_id.trim()) =>
        {
            return GroupAccess::Skip("group is not on the allow-list")
        }
        GroupPolicy::Allowlist | GroupPolicy::Open => {}
    }
    // A reply to the bot does not stand in for authorisation, and the `*`
    // that opens DMs does not open groups: group senders must be listed.
    if !listed(allow, &facts.sender_id) {
        return GroupAccess::Skip("sender is not on the allow-list");
    }
    if config.require_mention && !(facts.mentioned || facts.reply_to_bot) {
        return GroupAccess::Skip("not mentioned");
    }
    GroupAccess::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn dm_policies() {
        let paired = list(&["42"]);
        assert_eq!(dm_access(DmPolicy::Pairing, &paired, "42"), DmAccess::Allow);
        assert_eq!(
            dm_access(DmPolicy::Pairing, &paired, "7"),
            DmAccess::NeedsPairing
        );
        assert_eq!(
            dm_access(DmPolicy::Allowlist, &paired, "42"),
            DmAccess::Allow
        );
        assert!(matches!(
            dm_access(DmPolicy::Allowlist, &paired, "7"),
            DmAccess::Deny(_)
        ));
        assert!(matches!(
            dm_access(DmPolicy::Disabled, &paired, "42"),
            DmAccess::Deny(_)
        ));
        // Open requires an explicit `*`; a populated list is not enough.
        assert!(matches!(
            dm_access(DmPolicy::Open, &paired, "7"),
            DmAccess::Deny(_)
        ));
        assert_eq!(
            dm_access(DmPolicy::Open, &list(&["*"]), "7"),
            DmAccess::Allow
        );
        // `*` does not turn the allowlist policy into open.
        assert!(matches!(
            dm_access(DmPolicy::Allowlist, &list(&["*"]), "7"),
            DmAccess::Deny(_)
        ));
        assert!(matches!(
            dm_access(DmPolicy::Allowlist, &paired, " "),
            DmAccess::Deny(_)
        ));
    }

    fn group_config(policy: GroupPolicy) -> ChannelConfig {
        ChannelConfig {
            group_policy: policy,
            groups: list(&["-100"]),
            ..ChannelConfig::default()
        }
    }

    fn facts(sender: &str, mentioned: bool, reply: bool) -> GroupFacts {
        GroupFacts {
            group_id: "-100".into(),
            sender_id: sender.into(),
            mentioned,
            reply_to_bot: reply,
        }
    }

    #[test]
    fn group_gating_order() {
        let allow = list(&["42"]);
        let cfg = group_config(GroupPolicy::Disabled);
        assert_eq!(
            group_access(&cfg, &allow, &facts("42", true, false)),
            GroupAccess::Skip("groups are disabled")
        );

        let cfg = group_config(GroupPolicy::Allowlist);
        let mut other = facts("42", true, false);
        other.group_id = "-200".into();
        assert_eq!(
            group_access(&cfg, &allow, &other),
            GroupAccess::Skip("group is not on the allow-list")
        );
        assert_eq!(
            group_access(&cfg, &allow, &facts("42", true, false)),
            GroupAccess::Allow
        );
        assert_eq!(
            group_access(&cfg, &allow, &facts("42", false, false)),
            GroupAccess::Skip("not mentioned")
        );
        assert_eq!(
            group_access(&cfg, &allow, &facts("42", false, true)),
            GroupAccess::Allow
        );

        let cfg = group_config(GroupPolicy::Open);
        assert_eq!(group_access(&cfg, &allow, &other), GroupAccess::Allow);
    }

    #[test]
    fn reply_to_bot_does_not_authorise_the_sender() {
        let cfg = group_config(GroupPolicy::Allowlist);
        let allow = list(&["42"]);
        assert_eq!(
            group_access(&cfg, &allow, &facts("7", false, true)),
            GroupAccess::Skip("sender is not on the allow-list")
        );
        assert_eq!(
            group_access(&cfg, &allow, &facts("7", true, true)),
            GroupAccess::Skip("sender is not on the allow-list")
        );
    }

    #[test]
    fn mention_not_required_when_disabled() {
        let mut cfg = group_config(GroupPolicy::Allowlist);
        cfg.require_mention = false;
        assert_eq!(
            group_access(&cfg, &list(&["42"]), &facts("42", false, false)),
            GroupAccess::Allow
        );
        // `*` (for open DMs) does not admit unknown group senders.
        assert_eq!(
            group_access(&cfg, &list(&["*"]), &facts("9", false, false)),
            GroupAccess::Skip("sender is not on the allow-list")
        );
    }
}
