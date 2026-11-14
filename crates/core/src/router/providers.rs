// Design adapted from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/providers/registry/* @ 1855aa4
//! Provider connections and their API keys.
//!
//! A [`Connection`] is one account at one provider (`openai`, `anthropic`,
//! `gemini`, `openrouter`, …). Non-secret settings live in the
//! `llm_connections` table; the API key lives in the OS keychain, one entry
//! per connection id ([`KeySource`]). Several connections may share a
//! provider: they are the provider's accounts, tried by `priority`.

use super::settings::{normalize_base_url, KeyringSecret, SecretError, SecretStore};
use crate::store::ConnectionRow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// Wire format a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// `POST {base}/chat/completions` with a bearer key.
    OpenAi,
    /// `POST {base}/messages` (Anthropic Messages API).
    Anthropic,
    /// `POST {base}/models/{model}:streamGenerateContent` (Gemini API).
    Gemini,
}

impl ProviderKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
}

/// A known provider: wire format, default endpoint, whether a key is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    pub needs_key: bool,
    /// Suggested model ids, shown in the UI.
    pub models: &'static [&'static str],
}

/// Providers the gateway knows. `custom` is any OpenAI-compatible endpoint.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "openai",
        label: "OpenAI",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.openai.com/v1",
        needs_key: true,
        models: &[
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "o4-mini",
        ],
    },
    Preset {
        id: "anthropic",
        label: "Anthropic",
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        needs_key: true,
        models: &[
            "claude-sonnet-4-5",
            "claude-opus-4-1",
            "claude-haiku-4-5",
            "claude-3-5-haiku-latest",
        ],
    },
    Preset {
        id: "gemini",
        label: "Google Gemini",
        kind: ProviderKind::Gemini,
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        needs_key: true,
        models: &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
        ],
    },
    Preset {
        id: "openrouter",
        label: "OpenRouter",
        kind: ProviderKind::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        needs_key: true,
        models: &["openai/gpt-4o-mini", "anthropic/claude-sonnet-4.5"],
    },
    Preset {
        id: "deepseek",
        label: "DeepSeek",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.deepseek.com/v1",
        needs_key: true,
        models: &["deepseek-chat", "deepseek-reasoner"],
    },
    Preset {
        id: "groq",
        label: "Groq",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.groq.com/openai/v1",
        needs_key: true,
        models: &["llama-3.3-70b-versatile", "openai/gpt-oss-120b"],
    },
    Preset {
        id: "xai",
        label: "xAI",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.x.ai/v1",
        needs_key: true,
        models: &["grok-4", "grok-3-mini"],
    },
    Preset {
        id: "mistral",
        label: "Mistral",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.mistral.ai/v1",
        needs_key: true,
        models: &["mistral-large-latest", "codestral-latest"],
    },
    Preset {
        id: "together",
        label: "Together AI",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.together.xyz/v1",
        needs_key: true,
        models: &["meta-llama/Llama-3.3-70B-Instruct-Turbo"],
    },
    Preset {
        id: "fireworks",
        label: "Fireworks AI",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.fireworks.ai/inference/v1",
        needs_key: true,
        models: &["accounts/fireworks/models/llama-v3p3-70b-instruct"],
    },
    Preset {
        id: "ollama",
        label: "Ollama (local)",
        kind: ProviderKind::OpenAi,
        base_url: "http://127.0.0.1:11434/v1",
        needs_key: false,
        models: &["llama3.2", "qwen2.5-coder"],
    },
    Preset {
        id: "lmstudio",
        label: "LM Studio (local)",
        kind: ProviderKind::OpenAi,
        base_url: "http://127.0.0.1:1234/v1",
        needs_key: false,
        models: &[],
    },
    Preset {
        id: "9router",
        label: "9Router (external, optional)",
        kind: ProviderKind::OpenAi,
        base_url: super::settings::DEFAULT_BASE_URL,
        needs_key: false,
        models: &[],
    },
    Preset {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        kind: ProviderKind::OpenAi,
        base_url: "http://127.0.0.1:8000/v1",
        needs_key: false,
        models: &[],
    },
];

/// Preset for a provider id, if known.
#[must_use]
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Id of the implicit connection built from the legacy `router.*` settings.
pub const LEGACY_ROUTER_ID: &str = "router";

/// Non-secret connection settings (the `data` JSON column).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionData {
    /// Overrides the preset endpoint (required for `custom`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Models offered in pickers for this connection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

/// A usable provider account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    /// Provider id (`openai`, `anthropic`, a preset id, or `custom`).
    pub provider: String,
    pub auth_type: String,
    pub name: String,
    pub priority: i64,
    pub is_active: bool,
    pub kind: ProviderKind,
    /// Effective endpoint without a trailing slash.
    pub base_url: String,
    pub models: Vec<String>,
    pub needs_key: bool,
}

impl Connection {
    /// Build from a stored row, applying the provider preset.
    ///
    /// # Errors
    /// Returns a message for an invalid base URL or `data` JSON.
    pub fn from_row(row: &ConnectionRow) -> Result<Self, String> {
        let data: ConnectionData = serde_json::from_str(&row.data)
            .map_err(|e| format!("connection {}: bad data: {e}", row.id))?;
        let preset = preset(&row.provider).or_else(|| preset("custom"));
        let kind = preset.map_or(ProviderKind::OpenAi, |p| p.kind);
        let base = data
            .base_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .or(preset.map(|p| p.base_url))
            .unwrap_or_default();
        Ok(Self {
            id: row.id.clone(),
            provider: row.provider.clone(),
            auth_type: row.auth_type.clone(),
            name: row.name.clone(),
            priority: row.priority,
            is_active: row.is_active,
            kind,
            base_url: normalize_base_url(base)?,
            models: data.models,
            needs_key: preset.is_some_and(|p| p.needs_key),
        })
    }

    /// `{base}/{path}`.
    #[must_use]
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    /// Whether `name` addresses this connection (provider id, id or name).
    #[must_use]
    pub fn answers_to(&self, name: &str) -> bool {
        self.provider == name || self.id == name || self.name.eq_ignore_ascii_case(name)
    }
}

/// Where connection API keys are kept.
pub trait KeySource: Send + Sync {
    /// Key for a connection, if one is stored.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain is unavailable.
    fn get(&self, connection_id: &str) -> Result<Option<String>, SecretError>;

    /// Store (replace) a connection's key.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain rejects the write.
    fn set(&self, connection_id: &str, key: &str) -> Result<(), SecretError>;

    /// Remove a connection's key; a missing key is not an error.
    ///
    /// # Errors
    /// Returns [`SecretError`] if the keychain rejects the delete.
    fn delete(&self, connection_id: &str) -> Result<(), SecretError>;
}

/// OS keychain: one [`KeyringSecret`] per connection
/// (account `provider:<id>`; the legacy router keeps `router-api-key`).
#[derive(Debug, Clone, Default)]
pub struct KeychainKeys;

impl KeychainKeys {
    fn entry(connection_id: &str) -> KeyringSecret {
        if connection_id == LEGACY_ROUTER_ID {
            KeyringSecret::default()
        } else {
            KeyringSecret::for_connection(connection_id)
        }
    }
}

impl KeySource for KeychainKeys {
    fn get(&self, connection_id: &str) -> Result<Option<String>, SecretError> {
        Self::entry(connection_id).get()
    }
    fn set(&self, connection_id: &str, key: &str) -> Result<(), SecretError> {
        Self::entry(connection_id).set(key)
    }
    fn delete(&self, connection_id: &str) -> Result<(), SecretError> {
        Self::entry(connection_id).delete()
    }
}

/// In-memory keys for tests and keychain-less environments.
#[derive(Debug, Default)]
pub struct MemoryKeys(Mutex<HashMap<String, String>>);

impl MemoryKeys {
    #[must_use]
    pub fn with(entries: &[(&str, &str)]) -> Self {
        Self(Mutex::new(
            entries
                .iter()
                .map(|(id, key)| ((*id).to_string(), (*key).to_string()))
                .collect(),
        ))
    }
}

impl KeySource for MemoryKeys {
    fn get(&self, connection_id: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(connection_id)
            .cloned())
    }
    fn set(&self, connection_id: &str, key: &str) -> Result<(), SecretError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(connection_id.to_string(), key.to_string());
        Ok(())
    }
    fn delete(&self, connection_id: &str) -> Result<(), SecretError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(connection_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(provider: &str, data: &str) -> ConnectionRow {
        ConnectionRow {
            id: "c1".into(),
            provider: provider.into(),
            auth_type: "api_key".into(),
            name: "Main".into(),
            priority: 1,
            is_active: true,
            data: data.into(),
        }
    }

    #[test]
    fn presets_fill_kind_and_endpoint() {
        let anthropic = Connection::from_row(&row("anthropic", "{}")).expect("conn");
        assert_eq!(anthropic.kind, ProviderKind::Anthropic);
        assert_eq!(
            anthropic.endpoint("messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert!(anthropic.needs_key);
        let ollama = Connection::from_row(&row("ollama", "{}")).expect("conn");
        assert_eq!(ollama.kind, ProviderKind::OpenAi);
        assert!(!ollama.needs_key);
        let custom = Connection::from_row(&row(
            "custom",
            r#"{"base_url":"http://localhost:9/v1/","models":["m"]}"#,
        ))
        .expect("conn");
        assert_eq!(custom.base_url, "http://localhost:9/v1");
        assert_eq!(custom.models, ["m"]);
        assert!(Connection::from_row(&row("custom", r#"{"base_url":"ftp://x"}"#)).is_err());
        assert!(
            custom.answers_to("custom") && custom.answers_to("main") && custom.answers_to("c1")
        );
    }

    #[test]
    fn memory_keys_are_per_connection() {
        let keys = MemoryKeys::with(&[("a", "ka")]);
        keys.set("b", "kb").expect("set");
        assert_eq!(keys.get("a").expect("get").as_deref(), Some("ka"));
        assert_eq!(keys.get("b").expect("get").as_deref(), Some("kb"));
        keys.delete("a").expect("delete");
        assert_eq!(keys.get("a").expect("get"), None);
    }
}
