//! Where bot tokens live: the OS keychain, one entry per channel
//! (`dev.nextagent.desktop` / `channel:<id>`), through the core's
//! keychain seam. Never SQLite, never files, never the UI after saving.

use next_agent_core::router::{KeyringSecret, SecretStore};

/// The keychain entry holding a channel's bot token.
#[must_use]
pub fn token_entry(channel_id: &str) -> KeyringSecret {
    KeyringSecret::new("dev.nextagent.desktop", format!("channel:{channel_id}"))
}

/// Read a token from `store`, trimmed; `None` if missing or empty.
///
/// # Errors
/// The keychain is unavailable.
pub fn read_token(store: &dyn SecretStore) -> Result<Option<String>, String> {
    Ok(store
        .get()
        .map_err(|e| e.to_string())?
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty()))
}
