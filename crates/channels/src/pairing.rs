// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/pairing/pairing-store-model.ts, src/pairing/pairing-messages.ts @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! DM pairing: unknown senders get a short code that the owner approves in
//! the desktop app (Settings → Channels). Pure logic over the pending list;
//! persistence is [`crate::state::ChannelState`].
//!
//! - A code expires after [`PAIRING_TTL_SECS`] (1 hour).
//! - Messaging again does not issue a new code; the pending one is repeated.
//! - At most [`PAIRING_MAX_PENDING`] requests wait per channel account.
//!   Stricter than OpenClaw: when full, new senders are refused instead of
//!   evicting the oldest request, so a flood cannot push out a real one.

use serde::{Deserialize, Serialize};

pub const PAIRING_TTL_SECS: u64 = 60 * 60;
pub const PAIRING_MAX_PENDING: usize = 3;
const CODE_LENGTH: usize = 8;
/// Upper case without the ambiguous `0 O 1 I`.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// One sender waiting for the owner's approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub channel_id: String,
    pub sender_id: String,
    /// Where to send the "approved" notice (the DM chat).
    pub chat_id: String,
    /// Display name (`@user`, first name); untrusted, for the owner's eyes.
    pub label: String,
    pub code: String,
    /// Unix seconds.
    pub created_at: u64,
    pub last_seen_at: u64,
}

impl PairingRequest {
    #[must_use]
    pub fn expires_at(&self) -> u64 {
        self.created_at + PAIRING_TTL_SECS
    }

    #[must_use]
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at()
    }
}

/// What to tell an unknown sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingOutcome {
    /// A fresh code was created.
    Issued(String),
    /// The sender already has a pending, unexpired code.
    Pending(String),
    /// Too many requests are already waiting.
    Full,
}

/// Drop expired requests.
pub fn prune(pending: &mut Vec<PairingRequest>, now: u64) {
    pending.retain(|r| !r.is_expired(now));
}

/// Record a DM from an unknown sender. `pending` must already be pruned.
/// `code` produces a random code (injected for tests).
pub fn request(
    pending: &mut Vec<PairingRequest>,
    template: PairingRequest,
    now: u64,
    code: &mut dyn FnMut() -> String,
) -> PairingOutcome {
    prune(pending, now);
    if let Some(existing) = pending
        .iter_mut()
        .find(|r| r.sender_id == template.sender_id)
    {
        existing.last_seen_at = now;
        return PairingOutcome::Pending(existing.code.clone());
    }
    if pending.len() >= PAIRING_MAX_PENDING {
        return PairingOutcome::Full;
    }
    let mut fresh = code();
    for _ in 0..16 {
        if !pending.iter().any(|r| r.code == fresh) {
            break;
        }
        fresh = code();
    }
    pending.push(PairingRequest {
        code: fresh.clone(),
        created_at: now,
        last_seen_at: now,
        ..template
    });
    PairingOutcome::Issued(fresh)
}

/// Remove and return the unexpired request with `code` (case-insensitive).
pub fn take(pending: &mut Vec<PairingRequest>, code: &str, now: u64) -> Option<PairingRequest> {
    prune(pending, now);
    let code = code.trim().to_ascii_uppercase();
    let index = pending.iter().position(|r| r.code == code)?;
    Some(pending.remove(index))
}

/// A random human-friendly code from the OS random source.
///
/// # Panics
/// Only if the OS random source is unavailable, which is unrecoverable.
#[must_use]
pub fn random_code() -> String {
    let mut bytes = [0u8; CODE_LENGTH];
    getrandom::getrandom(&mut bytes).expect("OS random source");
    bytes
        .iter()
        // 256 is a multiple of 32, so the modulo has no bias.
        .map(|b| char::from(CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()]))
        .collect()
}

/// Reply for an unknown sender. Points at the desktop app, never at a chat
/// command: approvals happen only on the desktop.
#[must_use]
pub fn pairing_reply(outcome: &PairingOutcome, sender_id: &str) -> String {
    match outcome {
        PairingOutcome::Issued(code) | PairingOutcome::Pending(code) => format!(
            "Next Agent: this chat is not paired yet.\n\nYour id: {sender_id}\nPairing code: {code}\n\nAsk the owner to approve it in the Next Agent desktop app (Settings → Channels). The code expires in 1 hour."
        ),
        PairingOutcome::Full => "Next Agent: too many pairing requests are waiting. Please try again later.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(sender: &str) -> PairingRequest {
        PairingRequest {
            channel_id: "ch_1".into(),
            sender_id: sender.into(),
            chat_id: sender.into(),
            label: format!("@{sender}"),
            code: String::new(),
            created_at: 0,
            last_seen_at: 0,
        }
    }

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("CODE{n:04}")
        }
    }

    #[test]
    fn issues_once_and_repeats_the_pending_code() {
        let mut pending = Vec::new();
        let mut code = counter();
        assert_eq!(request(&mut pending, template("a"), 100, &mut code), PairingOutcome::Issued("CODE0001".into()));
        assert_eq!(request(&mut pending, template("a"), 200, &mut code), PairingOutcome::Pending("CODE0001".into()));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].last_seen_at, 200);
        assert_eq!(pending[0].created_at, 100);
    }

    #[test]
    fn caps_pending_requests_at_three_without_evicting() {
        let mut pending = Vec::new();
        let mut code = counter();
        for sender in ["a", "b", "c"] {
            assert!(matches!(request(&mut pending, template(sender), 10, &mut code), PairingOutcome::Issued(_)));
        }
        assert_eq!(request(&mut pending, template("d"), 11, &mut code), PairingOutcome::Full);
        assert_eq!(pending.len(), PAIRING_MAX_PENDING);
        assert!(pending.iter().all(|r| r.sender_id != "d"));
        // Existing senders still get their code back.
        assert!(matches!(request(&mut pending, template("b"), 12, &mut code), PairingOutcome::Pending(_)));
    }

    #[test]
    fn codes_expire_after_one_hour() {
        let mut pending = Vec::new();
        let mut code = counter();
        request(&mut pending, template("a"), 1_000, &mut code);
        let first = pending[0].code.clone();
        // Just before expiry: still approvable.
        assert!(take(&mut pending.clone(), &first, 1_000 + PAIRING_TTL_SECS - 1).is_some());
        // At expiry: gone, and the sender gets a fresh code.
        assert!(take(&mut pending.clone(), &first, 1_000 + PAIRING_TTL_SECS).is_none());
        let outcome = request(&mut pending, template("a"), 1_000 + PAIRING_TTL_SECS, &mut code);
        assert_eq!(outcome, PairingOutcome::Issued("CODE0002".into()));
    }

    #[test]
    fn expiry_frees_a_slot_under_the_cap() {
        let mut pending = Vec::new();
        let mut code = counter();
        for (i, sender) in ["a", "b", "c"].iter().enumerate() {
            request(&mut pending, template(sender), i as u64, &mut code);
        }
        assert_eq!(request(&mut pending, template("d"), 100, &mut code), PairingOutcome::Full);
        let outcome = request(&mut pending, template("d"), PAIRING_TTL_SECS, &mut code);
        assert!(matches!(outcome, PairingOutcome::Issued(_)));
        assert!(pending.iter().all(|r| r.sender_id != "a"));
    }

    #[test]
    fn take_is_case_insensitive_and_single_use() {
        let mut pending = Vec::new();
        let mut code = counter();
        request(&mut pending, template("a"), 0, &mut code);
        assert_eq!(take(&mut pending, " code0001 ", 1).map(|r| r.sender_id), Some("a".into()));
        assert!(take(&mut pending, "CODE0001", 1).is_none());
    }

    #[test]
    fn random_codes_use_the_unambiguous_alphabet() {
        let code = random_code();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert!(!code.contains(['0', 'O', '1', 'I']));
    }

    #[test]
    fn reply_points_to_the_desktop() {
        let text = pairing_reply(&PairingOutcome::Issued("ABCD2345".into()), "42");
        assert!(text.contains("ABCD2345") && text.contains("desktop app"));
        assert!(!text.contains("/approve"));
    }
}
