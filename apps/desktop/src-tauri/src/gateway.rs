//! Built-in LLM gateway for the desktop app: provider connections (keys in
//! the OS keychain, never echoed back), combos, gateway settings and model
//! roles, usage reports, and the gateway the agent and status bar use.
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use crate::service::AppService;
use next_agent_core::router::gateway::Target;
use next_agent_core::router::providers::{preset, ConnectionData, LEGACY_ROUTER_ID};
use next_agent_core::router::settings::normalize_base_url;
use next_agent_core::router::{
    Connection, Gateway, GatewayConfig, GatewaySettings, KeySource, Pricing, StoreUsage, PRESETS,
    ROLES,
};
use next_agent_core::store::{ComboRow, ConnectionRow, UsageRow, UsageSummary, UsageTotals};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

/// Where connection keys live: the OS keychain (an in-memory store in tests).
#[must_use]
pub fn keys() -> Arc<dyn KeySource> {
    #[cfg(test)]
    {
        use std::sync::OnceLock;
        static TEST_KEYS: OnceLock<Arc<next_agent_core::router::MemoryKeys>> = OnceLock::new();
        let keys =
            TEST_KEYS.get_or_init(|| Arc::new(next_agent_core::router::MemoryKeys::default()));
        Arc::clone(keys) as Arc<dyn KeySource>
    }
    #[cfg(not(test))]
    {
        Arc::new(next_agent_core::router::KeychainKeys)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PresetDto {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub base_url: String,
    pub needs_key: bool,
    pub models: Vec<String>,
}

/// A connection as the UI sees it. The key itself is never included.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDto {
    pub id: String,
    pub provider: String,
    pub kind: String,
    pub name: String,
    pub priority: i64,
    pub is_active: bool,
    /// Effective endpoint.
    pub base_url: String,
    /// Only the user's override (empty = preset endpoint).
    pub base_url_override: String,
    pub models: Vec<String>,
    pub needs_key: bool,
    pub has_key: bool,
}

/// `gateway_connection_save` input.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectionInput {
    /// Empty = create.
    pub id: String,
    pub provider: String,
    pub name: String,
    pub priority: Option<i64>,
    pub is_active: Option<bool>,
    pub base_url: String,
    pub models: Vec<String>,
    /// Non-empty = store this key in the keychain.
    pub api_key: String,
    /// Remove the stored key.
    pub clear_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComboDto {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    /// `fallback` | `round-robin` | null (global setting).
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub sticky_limit: Option<i64>,
}

/// Something a role or default model can point at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetDto {
    pub value: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestResultDto {
    pub ok: bool,
    pub detail: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TotalsDto {
    pub key: String,
    pub requests: i64,
    pub failures: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost: f64,
}

impl From<&UsageTotals> for TotalsDto {
    fn from(t: &UsageTotals) -> Self {
        Self {
            key: t.key.clone(),
            requests: t.requests,
            failures: t.failures,
            prompt_tokens: t.prompt_tokens,
            completion_tokens: t.completion_tokens,
            cached_tokens: t.cached_tokens,
            reasoning_tokens: t.reasoning_tokens,
            cost: t.cost,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    pub ts: String,
    pub provider: String,
    pub model: String,
    pub role: String,
    pub combo: Option<String>,
    pub status: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub cost: f64,
    pub cost_known: bool,
    pub latency_ms: i64,
}

impl From<&UsageRow> for RecentDto {
    fn from(r: &UsageRow) -> Self {
        Self {
            ts: r.ts.clone().unwrap_or_default(),
            provider: r.provider.clone(),
            model: r.model.clone(),
            role: r.role.clone(),
            combo: r.combo.clone(),
            status: r.status,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            cached_tokens: r.cached_tokens,
            cost: r.cost,
            cost_known: r.cost_known,
            latency_ms: r.latency_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageDto {
    pub days: u32,
    pub totals: TotalsDto,
    pub by_provider: Vec<TotalsDto>,
    pub by_model: Vec<TotalsDto>,
    pub by_role: Vec<TotalsDto>,
    pub daily: Vec<TotalsDto>,
    pub recent: Vec<RecentDto>,
    pub saver_bytes_before: i64,
    pub saver_bytes_after: i64,
}

impl UsageDto {
    fn new(days: u32, s: &UsageSummary) -> Self {
        let all = |rows: &[UsageTotals]| rows.iter().map(TotalsDto::from).collect();
        Self {
            days,
            totals: TotalsDto::from(&s.totals),
            by_provider: all(&s.by_provider),
            by_model: all(&s.by_model),
            by_role: all(&s.by_role),
            daily: all(&s.daily),
            recent: s.recent.iter().map(RecentDto::from).collect(),
            saver_bytes_before: s.saver_bytes_before,
            saver_bytes_after: s.saver_bytes_after,
        }
    }
}

fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{prefix}_{nanos:x}")
}

fn dto(
    connection: &Connection,
    row: Option<&ConnectionRow>,
    keys: &dyn KeySource,
) -> ConnectionDto {
    let data: ConnectionData = row
        .and_then(|r| serde_json::from_str(&r.data).ok())
        .unwrap_or_default();
    ConnectionDto {
        id: connection.id.clone(),
        provider: connection.provider.clone(),
        kind: connection.kind.as_str().into(),
        name: connection.name.clone(),
        priority: connection.priority,
        is_active: connection.is_active,
        base_url: connection.base_url.clone(),
        base_url_override: data.base_url.unwrap_or_default(),
        models: connection.models.clone(),
        needs_key: connection.needs_key,
        has_key: keys
            .get(&connection.id)
            .ok()
            .flatten()
            .is_some_and(|k| !k.is_empty()),
    }
}

impl AppService {
    /// Gateway configuration from the store, with the user's pricing override
    /// (`pricing.json` next to the database) layered on the shipped table.
    pub(crate) fn gateway_config(&self) -> Result<GatewayConfig, String> {
        let config = GatewayConfig::load(&*self.store()?).map_err(|e| e.to_string())?;
        let user_file = self.db_path().with_file_name("pricing.json");
        let pricing = match std::fs::read_to_string(&user_file) {
            Ok(text) => match Pricing::from_json(&text) {
                Ok(user) => Pricing::builtin().with_override(user),
                Err(error) => {
                    eprintln!("ignoring pricing override: {error}");
                    Pricing::builtin()
                }
            },
            Err(_) => Pricing::builtin(),
        };
        Ok(config.with_pricing(pricing))
    }

    /// A gateway for `role`, recording usage to the database.
    pub(crate) fn gateway(&self, role: &str) -> Result<Gateway, String> {
        let gateway = Gateway::new(self.gateway_config()?, keys())
            .map_err(|e| e.to_string())?
            .with_role(role);
        Ok(match StoreUsage::open(self.db_path()) {
            Ok(usage) => gateway.with_usage(Arc::new(usage)),
            Err(error) => {
                eprintln!("usage will not be recorded: {error}");
                gateway
            }
        })
    }

    /// The gateway for the `chat` role when it can serve a request, or the
    /// reason it cannot. An implicit legacy router (`router.url`) must also be
    /// reachable, as before; built-in providers need only a key.
    pub(crate) fn chat_gateway(&self) -> Result<Gateway, String> {
        let gateway = self.gateway("chat")?;
        let config = gateway.config();
        let plan = config.resolve("", "chat").map_err(|_| {
            "no default model or chat role is set in Settings → Providers".to_string()
        })?;
        let keys = keys();
        let usable = |target: &Target| {
            config
                .accounts(&target.provider)
                .into_iter()
                .find(|account| {
                    !account.needs_key
                        || keys
                            .get(&account.id)
                            .ok()
                            .flatten()
                            .is_some_and(|k| !k.is_empty())
                })
        };
        let Some(account) = plan.targets.iter().find_map(usable) else {
            return Err(
                "no provider for the chat model has an API key (Settings → Providers)".into(),
            );
        };
        if account.id == LEGACY_ROUTER_ID {
            let healthy = self
                .health()
                .is_some_and(|h| h.status == "ok" || h.status == "degraded");
            if !healthy {
                return Err(
                    "the external router is not reachable and no built-in provider is configured"
                        .into(),
                );
            }
        }
        Ok(gateway)
    }

    /// The model or combo the `chat` role resolves to (recorded on tasks).
    pub(crate) fn chat_target(&self) -> String {
        self.gateway_config()
            .ok()
            .and_then(|c| c.role_target("chat"))
            .unwrap_or_default()
    }

    #[must_use]
    pub fn gateway_presets(&self) -> Vec<PresetDto> {
        PRESETS
            .iter()
            .map(|p| PresetDto {
                id: p.id.into(),
                label: p.label.into(),
                kind: p.kind.as_str().into(),
                base_url: p.base_url.into(),
                needs_key: p.needs_key,
                models: p.models.iter().map(|m| (*m).to_string()).collect(),
            })
            .collect()
    }

    pub fn gateway_connections(&self) -> Result<Vec<ConnectionDto>, String> {
        let rows = self
            .store()?
            .list_connections()
            .map_err(|e| e.to_string())?;
        let keys = keys();
        Ok(rows
            .iter()
            .filter_map(|row| {
                Connection::from_row(row)
                    .ok()
                    .map(|c| dto(&c, Some(row), keys.as_ref()))
            })
            .collect())
    }

    /// Create or update a connection; stores a new key in the keychain.
    pub fn gateway_connection_save(
        &self,
        input: &ConnectionInput,
    ) -> Result<ConnectionDto, String> {
        let provider = input.provider.trim();
        let preset = preset(provider).ok_or_else(|| format!("unknown provider `{provider}`"))?;
        let base_url = input.base_url.trim();
        if provider == "custom" && base_url.is_empty() {
            return Err("a custom provider needs a base URL".into());
        }
        let base_url = if base_url.is_empty() {
            None
        } else {
            Some(
                normalize_base_url(base_url)
                    .map_err(|_| "base URL must be an http or https URL".to_string())?,
            )
        };
        let id = if input.id.trim().is_empty() {
            new_id("conn")
        } else {
            input.id.trim().to_string()
        };
        let name = if input.name.trim().is_empty() {
            preset.label.to_string()
        } else {
            input.name.trim().to_string()
        };
        let data = ConnectionData {
            base_url,
            models: input
                .models
                .iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect(),
        };
        let row = ConnectionRow {
            id: id.clone(),
            provider: provider.to_string(),
            auth_type: "api_key".into(),
            name,
            priority: input.priority.unwrap_or(100),
            is_active: input.is_active.unwrap_or(true),
            data: serde_json::to_string(&data).map_err(|e| e.to_string())?,
        };
        let connection = Connection::from_row(&row)?;
        let keys = keys();
        if input.clear_key {
            keys.delete(&id).map_err(|e| e.to_string())?;
        } else if !input.api_key.trim().is_empty() {
            keys.set(&id, input.api_key.trim())
                .map_err(|e| e.to_string())?;
        }
        self.store()?
            .upsert_connection(&row)
            .map_err(|e| e.to_string())?;
        self.bump_router_generation();
        Ok(dto(&connection, Some(&row), keys.as_ref()))
    }

    /// Remove a connection and its keychain entry.
    pub fn gateway_connection_delete(&self, id: &str) -> Result<bool, String> {
        let removed = self
            .store()?
            .delete_connection(id)
            .map_err(|e| e.to_string())?;
        keys().delete(id).map_err(|e| e.to_string())?;
        self.bump_router_generation();
        Ok(removed)
    }

    /// List a connection's models (proves endpoint + key work).
    pub fn gateway_connection_test(&self, id: &str) -> Result<TestResultDto, String> {
        let gateway = self.gateway("chat")?;
        let account = gateway
            .config()
            .connections
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| format!("unknown connection {id}"))?;
        Ok(match gateway.list_models_blocking(&account) {
            Ok(models) => TestResultDto {
                ok: true,
                detail: format!("{} models available", models.len()),
                models,
            },
            Err(error) => TestResultDto {
                ok: false,
                detail: error.to_string(),
                models: Vec::new(),
            },
        })
    }

    pub fn gateway_combos(&self) -> Result<Vec<ComboDto>, String> {
        Ok(self
            .store()?
            .list_combos()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|row| ComboDto {
                id: row.id,
                name: row.name,
                models: serde_json::from_str(&row.models).unwrap_or_default(),
                strategy: row.strategy,
                sticky_limit: row.sticky_limit,
            })
            .collect())
    }

    pub fn gateway_combo_save(&self, input: &ComboDto) -> Result<ComboDto, String> {
        let name = input.name.trim();
        if name.is_empty() || name.contains('/') {
            return Err("a combo needs a name without `/`".into());
        }
        let models: Vec<String> = input
            .models
            .iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        if models.is_empty() {
            return Err("a combo needs at least one model".into());
        }
        let strategy = input.strategy.clone().filter(|s| !s.is_empty());
        if let Some(s) = &strategy {
            if next_agent_core::router::Strategy::parse(s).is_none() {
                return Err(format!(
                    "unknown strategy `{s}` (use fallback or round-robin)"
                ));
            }
        }
        let id = if input.id.trim().is_empty() {
            new_id("combo")
        } else {
            input.id.trim().to_string()
        };
        let row = ComboRow {
            id: id.clone(),
            name: name.to_string(),
            kind: "llm".into(),
            models: serde_json::to_string(&models).map_err(|e| e.to_string())?,
            strategy: strategy.clone(),
            sticky_limit: input.sticky_limit.filter(|n| *n > 0),
        };
        self.store()?
            .upsert_combo(&row)
            .map_err(|e| format!("could not save combo (is the name taken?): {e}"))?;
        self.bump_router_generation();
        Ok(ComboDto {
            id,
            name: row.name,
            models,
            strategy,
            sticky_limit: row.sticky_limit,
        })
    }

    pub fn gateway_combo_delete(&self, id: &str) -> Result<bool, String> {
        let removed = self.store()?.delete_combo(id).map_err(|e| e.to_string())?;
        self.bump_router_generation();
        Ok(removed)
    }

    pub fn gateway_settings_get(&self) -> Result<GatewaySettings, String> {
        GatewaySettings::load(&*self.store()?).map_err(|e| e.to_string())
    }

    pub fn gateway_settings_set(&self, settings: &GatewaySettings) -> Result<(), String> {
        if let Some(role) = settings.roles.keys().find(|r| !ROLES.contains(&r.as_str())) {
            return Err(format!("unknown role `{role}`"));
        }
        settings.save(&*self.store()?).map_err(|e| e.to_string())?;
        self.bump_router_generation();
        Ok(())
    }

    /// Models and combos a role or the default model can point at.
    pub fn gateway_targets(&self) -> Result<Vec<TargetDto>, String> {
        let config = self.gateway_config()?;
        let mut out: Vec<TargetDto> = config
            .combos
            .iter()
            .map(|c| TargetDto {
                value: c.name.clone(),
                label: format!("{} (combo: {})", c.name, c.models.join(" → ")),
                kind: "combo".into(),
            })
            .collect();
        for connection in config.connections.iter().filter(|c| c.is_active) {
            let suggested: Vec<String> = if connection.models.is_empty() {
                preset(&connection.provider).map_or_else(Vec::new, |p| {
                    p.models.iter().map(|m| (*m).to_string()).collect()
                })
            } else {
                connection.models.clone()
            };
            for model in suggested {
                let value = format!("{}/{model}", connection.provider);
                if !out.iter().any(|t| t.value == value) {
                    out.push(TargetDto {
                        label: format!("{value} ({})", connection.name),
                        value,
                        kind: "model".into(),
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn usage_summary(&self, days: u32) -> Result<UsageDto, String> {
        let summary = self
            .store()?
            .usage_summary(days, 20)
            .map_err(|e| e.to_string())?;
        Ok(UsageDto::new(days, &summary))
    }
}

type Svc<'a> = State<'a, Arc<AppService>>;

#[tauri::command]
#[must_use]
pub fn gateway_presets(service: Svc) -> Vec<PresetDto> {
    service.gateway_presets()
}

#[tauri::command]
pub fn gateway_connections(service: Svc) -> Result<Vec<ConnectionDto>, String> {
    service.gateway_connections()
}

#[tauri::command]
pub fn gateway_connection_save(
    service: Svc,
    input: ConnectionInput,
) -> Result<ConnectionDto, String> {
    service.gateway_connection_save(&input)
}

#[tauri::command]
pub fn gateway_connection_delete(service: Svc, id: String) -> Result<bool, String> {
    service.gateway_connection_delete(&id)
}

#[tauri::command]
pub async fn gateway_connection_test(
    service: Svc<'_>,
    id: String,
) -> Result<TestResultDto, String> {
    let service = Arc::clone(&service);
    tauri::async_runtime::spawn_blocking(move || service.gateway_connection_test(&id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn gateway_combos(service: Svc) -> Result<Vec<ComboDto>, String> {
    service.gateway_combos()
}

#[tauri::command]
pub fn gateway_combo_save(service: Svc, input: ComboDto) -> Result<ComboDto, String> {
    service.gateway_combo_save(&input)
}

#[tauri::command]
pub fn gateway_combo_delete(service: Svc, id: String) -> Result<bool, String> {
    service.gateway_combo_delete(&id)
}

#[tauri::command]
pub fn gateway_settings_get(service: Svc) -> Result<GatewaySettings, String> {
    service.gateway_settings_get()
}

#[tauri::command]
pub fn gateway_settings_set(service: Svc, settings: GatewaySettings) -> Result<(), String> {
    service.gateway_settings_set(&settings)
}

#[tauri::command]
pub fn gateway_targets(service: Svc) -> Result<Vec<TargetDto>, String> {
    service.gateway_targets()
}

#[tauri::command]
pub fn usage_summary(service: Svc, days: u32) -> Result<UsageDto, String> {
    service.usage_summary(days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_agent_core::router::RouterSettings;

    fn service() -> (tempfile::TempDir, AppService) {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = AppService::new(dir.path().join("db.sqlite")).expect("service");
        (dir, svc)
    }

    #[test]
    fn connections_store_keys_in_the_keychain_and_never_echo_them() {
        let (_dir, svc) = service();
        let saved = svc
            .gateway_connection_save(&ConnectionInput {
                provider: "anthropic".into(),
                api_key: "sk-ant-secret".into(),
                models: vec!["claude-sonnet-4-5".into(), " ".into()],
                ..ConnectionInput::default()
            })
            .expect("save");
        assert!(saved.has_key && saved.needs_key);
        assert_eq!(saved.name, "Anthropic");
        assert_eq!(saved.models, ["claude-sonnet-4-5"]);
        let listed =
            serde_json::to_string(&svc.gateway_connections().expect("list")).expect("json");
        assert!(!listed.contains("sk-ant-secret"), "keys are never returned");
        assert_eq!(
            keys().get(&saved.id).expect("get").as_deref(),
            Some("sk-ant-secret")
        );
        // The key is not in SQLite either.
        let rows = svc
            .store()
            .expect("store")
            .list_connections()
            .expect("rows");
        assert!(!rows[0].data.contains("sk-ant-secret"));
        // Editing without a key keeps it; clear_key removes it.
        let edited = svc
            .gateway_connection_save(&ConnectionInput {
                id: saved.id.clone(),
                provider: "anthropic".into(),
                name: "Work".into(),
                priority: Some(5),
                ..ConnectionInput::default()
            })
            .expect("edit");
        assert!(edited.has_key && edited.name == "Work" && edited.priority == 5);
        let cleared = svc
            .gateway_connection_save(&ConnectionInput {
                id: saved.id.clone(),
                provider: "anthropic".into(),
                clear_key: true,
                ..ConnectionInput::default()
            })
            .expect("clear");
        assert!(!cleared.has_key);
        assert!(svc.gateway_connection_delete(&saved.id).expect("delete"));
        assert_eq!(svc.gateway_connections().expect("list"), Vec::new());
    }

    #[test]
    fn connection_validation() {
        let (_dir, svc) = service();
        let bad = |input: ConnectionInput| svc.gateway_connection_save(&input).is_err();
        assert!(bad(ConnectionInput {
            provider: "nope".into(),
            ..ConnectionInput::default()
        }));
        assert!(bad(ConnectionInput {
            provider: "custom".into(),
            ..ConnectionInput::default()
        }));
        assert!(bad(ConnectionInput {
            provider: "custom".into(),
            base_url: "ftp://x".into(),
            ..ConnectionInput::default()
        }));
        let local = svc
            .gateway_connection_save(&ConnectionInput {
                provider: "ollama".into(),
                ..ConnectionInput::default()
            })
            .expect("ollama");
        assert!(!local.needs_key && local.base_url.ends_with(":11434/v1"));
    }

    #[test]
    fn combos_settings_roles_and_targets() {
        let (_dir, svc) = service();
        svc.gateway_connection_save(&ConnectionInput {
            provider: "openai".into(),
            ..ConnectionInput::default()
        })
        .expect("conn");
        assert!(svc
            .gateway_combo_save(&ComboDto {
                id: String::new(),
                name: String::new(),
                models: vec!["a/b".into()],
                strategy: None,
                sticky_limit: None
            })
            .is_err());
        assert!(svc
            .gateway_combo_save(&ComboDto {
                id: String::new(),
                name: "x".into(),
                models: vec![],
                strategy: None,
                sticky_limit: None
            })
            .is_err());
        assert!(svc
            .gateway_combo_save(&ComboDto {
                id: String::new(),
                name: "x".into(),
                models: vec!["a/b".into()],
                strategy: Some("smart".into()),
                sticky_limit: None
            })
            .is_err());
        let combo = svc
            .gateway_combo_save(&ComboDto {
                id: String::new(),
                name: "fast".into(),
                models: vec!["openai/gpt-4o-mini".into(), "openai/gpt-4o".into()],
                strategy: Some("round-robin".into()),
                sticky_limit: Some(2),
            })
            .expect("combo");
        assert_eq!(svc.gateway_combos().expect("list"), vec![combo.clone()]);
        let targets = svc.gateway_targets().expect("targets");
        assert_eq!(targets[0].value, "fast");
        assert!(targets.iter().any(|t| t.value == "openai/gpt-4o-mini"));

        let mut settings = svc.gateway_settings_get().expect("defaults");
        assert!(settings.token_saver && settings.rtk);
        settings.roles.insert("review".into(), "fast".into());
        settings.default_model = Some("openai/gpt-4o".into());
        svc.gateway_settings_set(&settings).expect("save");
        assert_eq!(svc.gateway_settings_get().expect("load"), settings);
        settings.roles.insert("poet".into(), "fast".into());
        assert!(svc.gateway_settings_set(&settings).is_err());
        assert_eq!(svc.chat_target(), "openai/gpt-4o");
        assert!(svc.gateway_combo_delete(&combo.id).expect("delete"));
    }

    #[test]
    fn chat_gateway_needs_a_model_and_a_key() {
        let (_dir, svc) = service();
        assert!(svc.chat_gateway().unwrap_err().contains("no default model"));
        let conn = svc
            .gateway_connection_save(&ConnectionInput {
                provider: "deepseek".into(),
                ..ConnectionInput::default()
            })
            .expect("conn");
        let settings = GatewaySettings {
            default_model: Some("deepseek/deepseek-chat".into()),
            ..GatewaySettings::default()
        };
        svc.gateway_settings_set(&settings).expect("settings");
        assert!(svc.chat_gateway().unwrap_err().contains("API key"));
        svc.gateway_connection_save(&ConnectionInput {
            id: conn.id.clone(),
            provider: "deepseek".into(),
            api_key: "k".into(),
            ..ConnectionInput::default()
        })
        .expect("key");
        assert!(svc.chat_gateway().is_ok(), "no external router needed");
        svc.gateway_connection_delete(&conn.id).expect("cleanup");
    }

    #[test]
    fn legacy_router_settings_still_resolve_but_need_the_router_up() {
        let (_dir, svc) = service();
        svc.setting_set("router.model", "gpt-x")
            .expect("legacy model");
        assert_eq!(svc.chat_target(), "9router/gpt-x");
        assert!(svc.chat_gateway().unwrap_err().contains("not reachable"));
        let settings = RouterSettings::load(&svc.store().expect("store")).expect("load");
        assert_eq!(settings.default_model.as_deref(), Some("gpt-x"));
    }

    /// The real agent, end to end, on a built-in provider (no external router):
    /// task → `BuiltinExecutor` → gateway → OpenAI-compatible provider.
    #[test]
    fn real_task_runs_through_the_gateway_without_a_router() {
        use crate::service::{Sink, TaskRequest};
        use crate::wire::StreamMessage;
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let server = runtime.block_on(MockServer::start());
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"All done.\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                    data: {\"choices\":[],\"usage\":{\"prompt_tokens\":42,\"completion_tokens\":3}}\n\n\
                    data: [DONE]\n\n";
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .and(header("authorization", "Bearer sk-e2e"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(body),
                )
                .mount(&server),
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let svc = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("service"));
        let conn = svc
            .gateway_connection_save(&ConnectionInput {
                provider: "custom".into(),
                base_url: format!("{}/v1", server.uri()),
                api_key: "sk-e2e".into(),
                ..ConnectionInput::default()
            })
            .expect("connection");
        let mut settings = GatewaySettings::default();
        settings
            .roles
            .insert("chat".into(), "custom/e2e-model".into());
        svc.gateway_settings_set(&settings).expect("settings");

        let ws = dir.path().join("plain");
        std::fs::create_dir(&ws).expect("mkdir");
        let opened = svc
            .workspace_open(ws.to_str().expect("utf8"))
            .expect("open");
        let log: Arc<std::sync::Mutex<Vec<StreamMessage>>> = Arc::default();
        let inner = Arc::clone(&log);
        let sink: Sink = Arc::new(move |m| inner.lock().expect("lock").push(m));
        let request = TaskRequest {
            workspace_id: opened.id,
            prompt: "Say you are done".into(),
            mode: "manual".into(),
        };
        let task = svc.task_start(&request, sink).expect("start");
        svc.task_wait(&task);

        let messages = log.lock().expect("lock").clone();
        assert!(
            !messages.iter().any(
                |m| matches!(m, StreamMessage::Notice { text, .. } if text.contains("demo agent"))
            ),
            "the real agent ran"
        );
        let events: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Event { event } => Some(event.clone()),
                _ => None,
            })
            .collect();
        assert!(
            events
                .iter()
                .any(|e| e["kind"] == "message" && e["data"]["text"] == "All done."),
            "{events:?}"
        );
        assert_eq!(events.last().expect("events")["kind"], "task_completed");
        let sent: serde_json::Value = runtime
            .block_on(server.received_requests())
            .and_then(|r| {
                r.first()
                    .map(|r| serde_json::from_slice(&r.body).unwrap_or_default())
            })
            .expect("one provider call");
        assert_eq!(sent["model"], "e2e-model");
        assert!(
            sent["tools"].as_array().is_some_and(|t| !t.is_empty()),
            "agent tools are sent"
        );
        let usage = svc.usage_summary(1).expect("usage");
        assert_eq!(usage.totals.prompt_tokens, 42);
        assert_eq!(usage.by_role[0].key, "chat");
        svc.gateway_connection_delete(&conn.id).expect("cleanup");
    }

    #[test]
    fn usage_summary_reads_the_rollups() {
        let (_dir, svc) = service();
        svc.store()
            .expect("store")
            .record_usage(&UsageRow {
                provider: "openai".into(),
                model: "gpt-4o".into(),
                role: "chat".into(),
                endpoint: "chat".into(),
                status: 200,
                prompt_tokens: 10,
                cost: 0.25,
                cost_known: true,
                ..UsageRow::default()
            })
            .expect("record");
        let usage = svc.usage_summary(7).expect("summary");
        assert_eq!(usage.totals.requests, 1);
        assert_eq!(usage.by_role[0].key, "chat");
        assert_eq!(usage.recent[0].model, "gpt-4o");
        let json = serde_json::to_value(&usage).expect("json");
        assert!(json.get("byProvider").is_some() && json["recent"][0].get("costKnown").is_some());
    }
}
