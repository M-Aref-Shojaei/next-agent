// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/routing/session-key.ts @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! Session routing: a pure function from (channel, account, peer, group,
//! thread) to a session key. Messages with the same key run in order; keys
//! run concurrently.
//!
//! Port of `buildAgentPeerSessionKey`, `resolveLinkedDirectPeerId` and
//! `resolveThreadSessionKeys` with a single agent (`main`) and our default
//! `dmScope` of `per-channel-peer`. Groups always get one session each.

use crate::config::DmScope;
use crate::types::ChatKind;
use std::collections::BTreeMap;

const AGENT: &str = "main";

/// Everything routing needs about one inbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInput<'a> {
    /// Channel kind (`telegram`).
    pub channel: &'a str,
    /// Bot account (our channel config id).
    pub account: &'a str,
    pub chat_kind: ChatKind,
    /// The sender for DMs, the group chat id for groups.
    pub peer_id: &'a str,
    /// Forum topic / thread inside a group.
    pub thread_id: Option<&'a str>,
}

/// The session key for `input`.
#[must_use]
pub fn session_key(
    input: &RouteInput<'_>,
    scope: DmScope,
    identity_links: &BTreeMap<String, Vec<String>>,
) -> String {
    let channel = normalize(input.channel, "unknown");
    let base = match input.chat_kind {
        ChatKind::Direct => {
            let mut peer = input.peer_id.trim().to_string();
            if scope != DmScope::Main {
                if let Some(linked) = linked_peer(identity_links, &channel, &peer) {
                    peer = linked;
                }
            }
            let peer = peer.to_lowercase();
            match scope {
                _ if peer.is_empty() => format!("agent:{AGENT}:main"),
                DmScope::Main => format!("agent:{AGENT}:main"),
                DmScope::PerPeer => format!("agent:{AGENT}:direct:{peer}"),
                DmScope::PerChannelPeer => format!("agent:{AGENT}:{channel}:direct:{peer}"),
                DmScope::PerAccountChannelPeer => format!(
                    "agent:{AGENT}:{channel}:{}:direct:{peer}",
                    normalize(input.account, "default")
                ),
            }
        }
        ChatKind::Group => format!(
            "agent:{AGENT}:{channel}:group:{}",
            normalize(input.peer_id, "unknown")
        ),
    };
    match input.thread_id.map(str::trim).filter(|t| !t.is_empty()) {
        Some(thread) => format!("{base}:thread:{}", thread.to_lowercase()),
        None => base,
    }
}

/// The canonical name a DM peer is linked to, matching `id` or
/// `channel:id` (case-insensitive).
fn linked_peer(links: &BTreeMap<String, Vec<String>>, channel: &str, peer: &str) -> Option<String> {
    if peer.is_empty() {
        return None;
    }
    let bare = peer.to_lowercase();
    let scoped = format!("{channel}:{bare}");
    links.iter().find_map(|(canonical, ids)| {
        let canonical = canonical.trim();
        (!canonical.is_empty()
            && ids.iter().any(|id| {
                let id = id.trim().to_lowercase();
                !id.is_empty() && (id == bare || id == scoped)
            }))
        .then(|| canonical.to_string())
    })
}

fn normalize(value: &str, fallback: &str) -> String {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm(peer: &str) -> RouteInput<'_> {
        RouteInput {
            channel: "Telegram",
            account: "ch_1",
            chat_kind: ChatKind::Direct,
            peer_id: peer,
            thread_id: None,
        }
    }

    fn key(input: &RouteInput<'_>, scope: DmScope) -> String {
        session_key(input, scope, &BTreeMap::new())
    }

    #[test]
    fn dm_scopes() {
        assert_eq!(key(&dm("42"), DmScope::Main), "agent:main:main");
        assert_eq!(key(&dm("42"), DmScope::PerPeer), "agent:main:direct:42");
        assert_eq!(
            key(&dm("42"), DmScope::PerChannelPeer),
            "agent:main:telegram:direct:42"
        );
        assert_eq!(
            key(&dm("42"), DmScope::PerAccountChannelPeer),
            "agent:main:telegram:ch_1:direct:42"
        );
        assert_eq!(key(&dm(" "), DmScope::PerChannelPeer), "agent:main:main");
    }

    #[test]
    fn default_scope_separates_people_and_channels() {
        let a = key(&dm("42"), DmScope::default());
        let b = key(&dm("43"), DmScope::default());
        let mut other_channel = dm("42");
        other_channel.channel = "discord";
        assert_ne!(a, b);
        assert_ne!(a, key(&other_channel, DmScope::default()));
    }

    #[test]
    fn identity_links_merge_one_person() {
        let mut links = BTreeMap::new();
        links.insert(
            "alice".to_string(),
            vec!["telegram:42".to_string(), "discord:99".to_string()],
        );
        let tg = session_key(&dm("42"), DmScope::PerPeer, &links);
        let mut discord = dm("99");
        discord.channel = "discord";
        assert_eq!(tg, "agent:main:direct:alice");
        assert_eq!(tg, session_key(&discord, DmScope::PerPeer, &links));
        // Scoped links do not match another channel's id.
        let mut wrong = dm("99");
        wrong.channel = "telegram";
        assert_eq!(
            session_key(&wrong, DmScope::PerPeer, &links),
            "agent:main:direct:99"
        );
        // `main` ignores links.
        assert_eq!(
            session_key(&dm("42"), DmScope::Main, &links),
            "agent:main:main"
        );
    }

    #[test]
    fn groups_get_their_own_session_whatever_the_dm_scope() {
        let group = RouteInput {
            chat_kind: ChatKind::Group,
            peer_id: "-100",
            ..dm("-100")
        };
        for scope in [DmScope::Main, DmScope::PerPeer, DmScope::PerChannelPeer] {
            assert_eq!(key(&group, scope), "agent:main:telegram:group:-100");
        }
        let topic = RouteInput {
            thread_id: Some("7"),
            ..group.clone()
        };
        assert_eq!(
            key(&topic, DmScope::default()),
            "agent:main:telegram:group:-100:thread:7"
        );
    }

    #[test]
    fn routing_is_pure() {
        let input = dm("42");
        assert_eq!(
            key(&input, DmScope::default()),
            key(&input, DmScope::default())
        );
    }
}
