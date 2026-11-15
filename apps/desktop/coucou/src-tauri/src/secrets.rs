// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/secrets.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
// API keys live in the Windows Credential Manager or, on Linux, the Secret
// Service (GNOME Keyring, KWallet) — never on disk and never in the front end — the island can only ask whether a key is present.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use keyring::Entry;

// Next Agent: renamed from Coucou (owner request): was "fr.louisraille.coucou" (see `migrate_legacy`).
const SERVICE: &str = "dev.nextagent.desktop.overlay";

/// Coucou's service name, which the Mac build used before the rename.
#[cfg(target_os = "macos")]
const LEGACY_SERVICE: &str = "fr.louisraille.coucou";

/// Next Agent: renamed from Coucou (owner request): moves the entries the Mac build stored under Coucou's service
/// name to Next Agent's, once (a marker file in the private config folder),
/// and deletes WhatsApp's old entries. Only on a Mac: on Windows and Linux that
/// service name is Coucou's own app's, whose entries are not ours to move.
#[cfg(target_os = "macos")]
fn migrate_legacy() {
    let marker = crate::settings::config_dir().join(".keychain-migrated");
    if marker.exists() {
        return;
    }
    let mut all_moved = true;
    for key in KNOWN_KEYS.iter().chain(INTERNAL_KEYS) {
        let (Ok(old), Ok(new)) = (Entry::new(LEGACY_SERVICE, key), Entry::new(SERVICE, key)) else { continue };
        let Ok(value) = old.get_password() else { continue };
        let moved = match new.get_password() {
            // Already set under the new name: the new one wins.
            Ok(_) => true,
            Err(_) => new.set_password(&value).is_ok() && new.get_password().ok().as_deref() == Some(value.as_str()),
        };
        if moved {
            let _ = old.delete_credential();
        } else {
            all_moved = false;
        }
    }
    // Next Agent: removed WhatsApp/Spotify (owner request): its old entries go.
    for key in ["whatsapp-url", "whatsapp-api-key", "whatsapp-webhook-secret"] {
        if let Ok(old) = Entry::new(LEGACY_SERVICE, key) {
            let _ = old.delete_credential();
        }
    }
    if all_moved && crate::platform::ensure_private_dir(&crate::settings::config_dir()).is_ok() {
        let _ = std::fs::write(&marker, b"");
    }
}

/// Every key Coucou may store. Anything outside this list is refused.
pub const KNOWN_KEYS: &[&str] = &[
    "anthropic-api-key",
    "openai-api-key",
    "google-api-key",
    "openrouter-api-key",
    "openai-compatible-key",
    "n8n-url",
    "n8n-api-key",
    "vercel-token",
    "github-token",
    "gitlab-url",
    "gitlab-token",
    "gitea-url",
    "gitea-token",
    "stripe-api-key",
    "resend-api-key",
    "notion-api-key",
    "calcom-api-key",
    // « OK Coucou »'s answers in an ElevenLabs voice.
    "elevenlabs-api-key",
];

fn entry(key: &str) -> Option<Entry> {
    if !KNOWN_KEYS.contains(&key) {
        return None;
    }
    test_store();
    Entry::new(SERVICE, key).ok()
}

/// Next Agent: tests never touch the real home folder (security review): under `cargo test` every entry is keyring's in-memory mock,
/// never the user's Keychain, Credential Manager or Secret Service.
fn test_store() {
    #[cfg(test)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| keyring::set_default_credential_builder(keyring::mock::default_credential_builder()));
    }
}

/// Whether each key is stored, as last seen. Each Secret Service lookup opens a
/// D-Bus session of its own (tens of milliseconds), and the settings ask about
/// every key on every page: a page switch took seconds. Only presence is kept,
/// never a value, and only what a lookup answered for sure.
static PRESENT: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn remember(key: &str, present: bool) {
    PRESENT.lock().unwrap().insert(key.to_string(), present);
}

pub fn get(key: &str) -> Option<String> {
    match entry(key)?.get_password() {
        Ok(value) if !value.is_empty() => {
            remember(key, true);
            Some(value)
        }
        Ok(_) | Err(keyring::Error::NoEntry) => {
            remember(key, false);
            None
        }
        // A locked or unreachable store says nothing about the key.
        Err(_) => None,
    }
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    let entry = entry(key).ok_or_else(|| format!("unknown key {key}"))?;
    if value.is_empty() {
        let _ = entry.delete_credential();
        remember(key, false);
        return Ok(());
    }
    entry.set_password(value).map_err(|e| e.to_string())?;
    remember(key, true);
    Ok(())
}

pub fn clear(key: &str) -> Result<(), String> {
    let entry = entry(key).ok_or_else(|| format!("unknown key {key}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            remember(key, false);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn present(key: &str) -> bool {
    if let Some(&known) = PRESENT.lock().unwrap().get(key) {
        return known;
    }
    get(key).is_some()
}

/// Looks every key up once, off the main thread, so the settings open on known answers.
pub fn warm_up() {
    std::thread::spawn(|| {
        // Next Agent: renamed from Coucou (owner request): before the first lookup.
        #[cfg(target_os = "macos")]
        migrate_legacy();
        for key in KNOWN_KEYS {
            let _ = get(key);
        }
    });
}

// ── Entries only Rust reads and writes ───────────────────────────────────────
//
// The phone link's certificate, key and pairing token, the relay's room, end-to-end key and access key.
// Next Agent: removed WhatsApp/Spotify (owner request): WhatsApp's gateway address, credential and webhook secret.
// They are not in KNOWN_KEYS
// on purpose: the island has `secret_present` / `secret_set` / `secret_clear`
// for those, and none of the three may be reachable from a webview.

/// Every entry of that kind. Anything outside this list is refused.
pub const INTERNAL_KEYS: &[&str] = &[
    "phone-link-cert",
    "phone-link-key",
    "phone-link-token",
    "phone-relay-room",
    "phone-relay-key",
    "phone-relay-access",
    // Next Agent: removed WhatsApp/Spotify (owner request): "whatsapp-url",
    // "whatsapp-api-key", "whatsapp-webhook-secret".
];

/// Whether the keystore would accept this key through `get_internal` / `set_internal`.
pub fn is_internal(key: &str) -> bool {
    INTERNAL_KEYS.contains(&key)
}

fn internal_entry(key: &str) -> Result<Entry, String> {
    if !is_internal(key) {
        return Err(format!("unknown key {key}"));
    }
    test_store();
    Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

pub fn get_internal(key: &str) -> Option<String> {
    internal_entry(key).ok()?.get_password().ok().filter(|v| !v.is_empty())
}

pub fn set_internal(key: &str, value: &str) -> Result<(), String> {
    let entry = internal_entry(key)?;
    if value.is_empty() {
        // An empty value means "forget it" (the relay's access key); some keystores refuse an empty password.
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_is_answered_from_memory_once_known() {
        remember("calcom-api-key", true);
        assert!(present("calcom-api-key"));
        remember("calcom-api-key", false);
        assert!(!present("calcom-api-key"));
    }

    #[test]
    fn the_phone_links_secrets_cannot_be_reached_from_the_island() {
        // The island's commands go through `get`/`set`/`clear`/`present`, which
        // only know KNOWN_KEYS.
        for key in INTERNAL_KEYS {
            assert!(!KNOWN_KEYS.contains(key), "{key}");
            assert!(entry(key).is_none(), "{key}");
            assert!(set(key, "x").is_err(), "{key}");
            assert!(clear(key).is_err(), "{key}");
            assert!(!present(key), "{key}");
        }
        // And the other way round: a key of the island is not an internal one.
        assert!(set_internal("github-token", "x").is_err());
    }

    /// Windows Credential Manager caps a secret's size. The phone link stores its certificate (about
    /// 600 hex characters) and key there, so prove that a secret of that size round-trips through the
    /// real store. A separate service name: this can never touch a user's real entries.
    #[cfg(windows)]
    #[test]
    fn a_certificate_sized_secret_round_trips_through_credential_manager() {
        let entry = keyring::Entry::new("dev.nextagent.desktop.overlay.test", "phone-link-size-check").unwrap();
        let big: String = (0..1_200).map(|i| char::from(b"0123456789abcdef"[i % 16])).collect();
        entry.set_password(&big).expect("Credential Manager refused a certificate-sized secret");
        let back = entry.get_password().expect("could not read it back");
        let _ = entry.delete_credential();
        assert_eq!(back, big);
    }
}
