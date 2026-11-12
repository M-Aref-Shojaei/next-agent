//! Router settings: base URL and default model live in the metadata
//! database; the API key lives in the OS keychain, never in SQLite.

use crate::store::{Store, StoreError};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// 9Router's default OpenAI-compatible endpoint.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128/v1";

const KEY_BASE_URL: &str = "router.base_url";
const KEY_DEFAULT_MODEL: &str = "router.default_model";

/// User-editable router configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterSettings {
    /// Base URL including the `/v1` suffix, without a trailing slash.
    pub base_url: String,
    /// Model used when a task does not override it.
    pub default_model: Option<String>,
    /// Timeout for the health probe (spec: 3 s).
    pub health_timeout: Duration,
    /// A healthy answer slower than this reports `degraded`.
    pub slow_threshold: Duration,
}

impl Default for RouterSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.into(),
            default_model: None,
            health_timeout: Duration::from_secs(3),
            slow_threshold: Duration::from_millis(1_500),
        }
    }
}

impl RouterSettings {
    /// Settings with a validated, normalized base URL.
    ///
    /// # Errors
    /// Returns a message if the URL is not `http(s)://…`.
    pub fn with_base_url(mut self, url: &str) -> Result<Self, String> {
        self.base_url = normalize_base_url(url)?;
        Ok(self)
    }

    /// Load from the `settings` table, falling back to defaults.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the database read fails.
    pub fn load(store: &Store) -> Result<Self, StoreError> {
        let mut settings = Self::default();
        if let Some(url) = read_string(store, KEY_BASE_URL)? {
            if let Ok(url) = normalize_base_url(&url) {
                settings.base_url = url;
            }
        }
        settings.default_model = read_string(store, KEY_DEFAULT_MODEL)?.filter(|m| !m.is_empty());
        Ok(settings)
    }

    /// Persist base URL and default model (values stored as JSON strings).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn save(&self, store: &Store) -> Result<(), StoreError> {
        store.set_setting(
            KEY_BASE_URL,
            &serde_json::Value::String(self.base_url.clone()).to_string(),
        )?;
        store.set_setting(
            KEY_DEFAULT_MODEL,
            &serde_json::Value::String(self.default_model.clone().unwrap_or_default()).to_string(),
        )
    }

    /// `{base}/{path}`.
    #[must_use]
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}

fn read_string(store: &Store, key: &str) -> Result<Option<String>, StoreError> {
    Ok(store
        .get_setting(key)?
        .map(|raw| serde_json::from_str::<String>(&raw).unwrap_or(raw)))
}

/// Trim whitespace and trailing slashes; require an http(s) scheme.
///
/// # Errors
/// Returns a message for anything that is not `http://` or `https://`.
pub fn normalize_base_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .ok_or_else(|| format!("router URL must start with http:// or https:// (got `{url}`)"))?;
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return Err(format!("router URL has no host: `{url}`"));
    }
    Ok(trimmed.to_string())
}

/// Keychain failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretError(pub String);

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "keychain error: {}", self.0)
    }
}

impl std::error::Error for SecretError {}

/// Where the router API key is kept.
pub trait SecretStore: Send + Sync {
    /// Current key, if any.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain is unavailable.
    fn get(&self) -> Result<Option<String>, SecretError>;

    /// Store (replace) the key.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain rejects the write.
    fn set(&self, secret: &str) -> Result<(), SecretError>;

    /// Remove the key; removing a missing key is not an error.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain rejects the delete.
    fn delete(&self) -> Result<(), SecretError>;
}

/// OS keychain entry (macOS Keychain, Windows Credential Manager, Linux
/// kernel keyutils) via the `keyring` crate.
#[derive(Debug, Clone)]
pub struct KeyringSecret {
    service: String,
    account: String,
}

impl Default for KeyringSecret {
    fn default() -> Self {
        Self::new("dev.nextagent.desktop", "router-api-key")
    }
}

impl KeyringSecret {
    #[must_use]
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, &self.account).map_err(|e| SecretError(e.to_string()))
    }
}

impl SecretStore for KeyringSecret {
    fn get(&self) -> Result<Option<String>, SecretError> {
        match self.entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError(error.to_string())),
        }
    }

    fn set(&self, secret: &str) -> Result<(), SecretError> {
        self.entry()?
            .set_password(secret)
            .map_err(|e| SecretError(e.to_string()))
    }

    fn delete(&self) -> Result<(), SecretError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError(error.to_string())),
        }
    }
}

/// In-memory secret for tests and keychain-less environments.
#[derive(Debug, Default)]
pub struct MemorySecret(Mutex<Option<String>>);

impl MemorySecret {
    #[must_use]
    pub fn new(secret: Option<&str>) -> Self {
        Self(Mutex::new(secret.map(str::to_string)))
    }
}

impl SecretStore for MemorySecret {
    fn get(&self) -> Result<Option<String>, SecretError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn set(&self, secret: &str) -> Result<(), SecretError> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(secret.to_string());
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let settings = RouterSettings::default();
        assert_eq!(settings.base_url, "http://127.0.0.1:20128/v1");
        assert_eq!(settings.health_timeout, Duration::from_secs(3));
        assert_eq!(
            settings.endpoint("/models"),
            "http://127.0.0.1:20128/v1/models"
        );
    }

    #[test]
    fn base_url_is_validated_and_normalized() {
        assert_eq!(
            normalize_base_url(" http://localhost:9000/v1/ ").as_deref(),
            Ok("http://localhost:9000/v1")
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("http://").is_err());
        assert!(RouterSettings::default().with_base_url("nope").is_err());
    }

    #[test]
    fn settings_round_trip_through_the_store() {
        let store = Store::open_in_memory().expect("store");
        assert_eq!(
            RouterSettings::load(&store).expect("load"),
            RouterSettings::default()
        );
        let mut settings = RouterSettings::default()
            .with_base_url("https://router.example/v1/")
            .expect("url");
        settings.default_model = Some("gpt-x".into());
        settings.save(&store).expect("save");
        let loaded = RouterSettings::load(&store).expect("load");
        assert_eq!(loaded.base_url, "https://router.example/v1");
        assert_eq!(loaded.default_model.as_deref(), Some("gpt-x"));
        // The API key is never written to SQLite.
        assert!(store.get_setting("router.api_key").expect("get").is_none());
    }

    #[test]
    fn memory_secret_behaves_like_a_keychain() {
        let secret = MemorySecret::new(None);
        assert_eq!(secret.get(), Ok(None));
        secret.set("sk-test").expect("set");
        assert_eq!(secret.get(), Ok(Some("sk-test".into())));
        secret.delete().expect("delete");
        secret.delete().expect("idempotent delete");
        assert_eq!(secret.get(), Ok(None));
    }
}
