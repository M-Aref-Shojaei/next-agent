// Ported from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/services/combo.js, open-sse/services/accountFallback.js, open-sse/handlers/chat* @ 1855aa4
//! The built-in LLM gateway: resolves a model, role or combo to provider
//! accounts, runs the pipeline per attempt (translate → token savers → call →
//! translate back), falls through on failure, applies account cooldowns and
//! records usage. Implements [`CompletionClient`] for the agent and
//! [`HealthProbe`] for the status bar.
//!
//! Fall-through rules per model (one attempt each):
//! - 2xx stream with no content → next model, unless it is the last one or
//!   the finish reason says retrying is pointless (`length`, `content_filter`);
//! - 401/402/403 → cool the account down, try the provider's next account;
//! - 429 → lock the model on that account, try the next account;
//! - 502/503/504 → cool down; if that is ≤ 5 s, wait it out; next model;
//! - other 4xx (request-scoped) → remember the first, next model;
//! - transport errors and other 5xx → next model.
//!
//! When everything failed: the first request-scoped error, otherwise 503.
//!
//! A `smart` combo starts at the session's sticky model and updates it after
//! the answer (see [`super::smart`]); the session is the task id given by
//! [`Gateway::with_session`]. Calls without a
//! session share one sticky model per combo. Concurrent calls of one session
//! may overwrite each other's quality window (last write wins).

use super::client::{classify_status, describe};
use super::combo::{reorder_by_caps, required_caps, rotate, Combo, GatewayState, Strategy};
use super::pricing::Pricing;
use super::providers::{Connection, KeySource, ProviderKind, LEGACY_ROUTER_ID};
use super::savers::{self, prompts::Level, Format, SaverConfig, SaverReport};
use super::settings::RouterSettings;
use super::smart::{self, Sticky};
use super::translate::{anthropic, gemini, OpenAiDecoder, StreamDecoder, Usage};
use crate::llm::{ChatRequest, Completion, CompletionClient, LlmError, StreamAccumulator};
use crate::store::{StickyRow, Store, StoreError, SwitchRow, UsageRow};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use next_agent_process_manager::{HealthProbe, HealthReport, HealthStatus, ProbeFuture};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Jobs that can each be given their own model or combo.
pub const ROLES: [&str; 4] = ["chat", "design", "summary", "review"];
/// `settings` key holding [`GatewaySettings`] as JSON.
pub const SETTINGS_KEY: &str = "gateway.settings";
/// Cooldowns up to this long are waited out before moving on (5xx).
const SHORT_COOLDOWN: Duration = Duration::from_secs(5);
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// User-editable gateway settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GatewaySettings {
    /// `provider/model` or combo name used when a role has no assignment.
    pub default_model: Option<String>,
    /// Global token-saver switch.
    pub token_saver: bool,
    pub caveman: Option<Level>,
    pub ponytail: Option<Level>,
    pub rtk: bool,
    pub cache_control: bool,
    /// Strategy for combos that do not set their own.
    pub combo_strategy: Strategy,
    /// Role (`chat`, `design`, `summary`, `review`) → model or combo.
    pub roles: BTreeMap<String, String>,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            default_model: None,
            token_saver: true,
            caveman: None,
            ponytail: None,
            rtk: true,
            cache_control: true,
            combo_strategy: Strategy::Fallback,
            roles: BTreeMap::new(),
        }
    }
}

impl GatewaySettings {
    /// Load from the `settings` table (defaults when absent or unreadable).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the database read fails.
    pub fn load(store: &Store) -> Result<Self, StoreError> {
        Ok(store
            .get_setting(SETTINGS_KEY)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default())
    }

    /// Persist; empty role entries are dropped.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn save(&self, store: &Store) -> Result<(), StoreError> {
        let mut clean = self.clone();
        clean
            .roles
            .retain(|role, target| ROLES.contains(&role.as_str()) && !target.trim().is_empty());
        clean.default_model = clean.default_model.filter(|m| !m.trim().is_empty());
        let text = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
        store.set_setting(SETTINGS_KEY, &text)
    }

    fn saver_config(&self, per_request: bool) -> SaverConfig {
        SaverConfig {
            enabled: self.token_saver && per_request,
            caveman: self.caveman,
            ponytail: self.ponytail,
            rtk: self.rtk,
            cache_control: self.cache_control,
        }
    }
}

/// `provider/model` (the provider part may also be a connection id or name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub provider: String,
    pub model: String,
}

impl Target {
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

/// What a model name resolved to.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub combo: Option<Combo>,
    pub targets: Vec<Target>,
}

/// Snapshot of everything the gateway needs, loaded from the store.
#[derive(Debug, Clone, Default)]
pub struct GatewayConfig {
    pub connections: Vec<Connection>,
    pub combos: Vec<Combo>,
    pub settings: GatewaySettings,
    pub pricing: Pricing,
    /// `9router/<model>` from the legacy `router.default_model` setting.
    pub legacy_model: Option<String>,
}

impl GatewayConfig {
    /// Load connections, combos and settings. Rows that fail to parse are
    /// skipped with a warning. The legacy `router.url` / `router.model`
    /// settings become an implicit OpenAI-compatible connection named
    /// `router` when a legacy model is set.
    ///
    /// # Errors
    /// Returns [`StoreError`] if a database read fails.
    pub fn load(store: &Store) -> Result<Self, StoreError> {
        let mut connections: Vec<Connection> = store
            .list_connections()?
            .iter()
            .filter_map(|row| {
                Connection::from_row(row)
                    .map_err(
                        |e| tracing::warn!(target: "router", error = %e, "skipping connection"),
                    )
                    .ok()
            })
            .collect();
        let combos = store
            .list_combos()?
            .iter()
            .filter_map(|row| {
                Combo::from_row(row)
                    .map_err(|e| tracing::warn!(target: "router", error = %e, "skipping combo"))
                    .ok()
            })
            .collect();
        let legacy = RouterSettings::load(store)?;
        let mut legacy_model = None;
        if let Some(model) = legacy.default_model.clone() {
            if !connections.iter().any(|c| c.id == LEGACY_ROUTER_ID) {
                connections.push(Connection {
                    id: LEGACY_ROUTER_ID.into(),
                    provider: "9router".into(),
                    auth_type: "api_key".into(),
                    name: "9Router (router.url)".into(),
                    priority: 1_000,
                    is_active: true,
                    kind: ProviderKind::OpenAi,
                    base_url: legacy.base_url.clone(),
                    models: vec![model.clone()],
                    needs_key: false,
                });
            }
            legacy_model = Some(format!("9router/{model}"));
        }
        Ok(Self {
            connections,
            combos,
            settings: GatewaySettings::load(store)?,
            pricing: Pricing::builtin(),
            legacy_model,
        })
    }

    #[must_use]
    pub fn with_pricing(mut self, pricing: Pricing) -> Self {
        self.pricing = pricing;
        self
    }

    /// The model or combo a role uses: its assignment, else the `chat`
    /// assignment, else the default model, else the legacy router model.
    #[must_use]
    pub fn role_target(&self, role: &str) -> Option<String> {
        let pick = |key: &str| {
            self.settings
                .roles
                .get(key)
                .filter(|t| !t.trim().is_empty())
                .cloned()
        };
        pick(role)
            .or_else(|| pick("chat"))
            .or_else(|| {
                self.settings
                    .default_model
                    .clone()
                    .filter(|m| !m.trim().is_empty())
            })
            .or_else(|| self.legacy_model.clone())
    }

    /// Active accounts answering to a provider address, by priority.
    #[must_use]
    pub fn accounts(&self, provider: &str) -> Vec<&Connection> {
        let mut accounts: Vec<&Connection> = self
            .connections
            .iter()
            .filter(|c| c.is_active && c.answers_to(provider))
            .collect();
        accounts.sort_by_key(|c| c.priority);
        accounts
    }

    /// Parse `provider/model`. A bare model name works when exactly one
    /// active connection exists.
    #[must_use]
    pub fn parse_target(&self, name: &str) -> Option<Target> {
        let name = name.trim();
        if let Some((prefix, model)) = name.split_once('/') {
            if !model.is_empty() && self.connections.iter().any(|c| c.answers_to(prefix)) {
                return Some(Target {
                    provider: prefix.to_string(),
                    model: model.to_string(),
                });
            }
        }
        let active: Vec<&Connection> = self.connections.iter().filter(|c| c.is_active).collect();
        match active.as_slice() {
            [only] if !name.is_empty() => Some(Target {
                provider: only.provider.clone(),
                model: name.to_string(),
            }),
            _ => None,
        }
    }

    /// Resolve a requested model (empty = the role's target) to a plan.
    ///
    /// # Errors
    /// [`LlmError::Config`] when nothing is configured or the name is unknown.
    pub fn resolve(&self, requested: &str, role: &str) -> Result<Plan, LlmError> {
        let name = if requested.trim().is_empty() {
            self.role_target(role).ok_or_else(|| {
                LlmError::Config(
                    "no model is configured; add a provider in Settings → Providers and pick a default model"
                        .into(),
                )
            })?
        } else {
            requested.trim().to_string()
        };
        let bare = name.strip_prefix("combo:").unwrap_or(&name);
        if let Some(combo) = self.combos.iter().find(|c| c.name == bare) {
            let targets: Vec<Target> = combo
                .models
                .iter()
                .filter_map(|m| self.parse_target(m))
                .collect();
            if targets.is_empty() {
                return Err(LlmError::Config(format!(
                    "combo `{}` has no model with a configured provider",
                    combo.name
                )));
            }
            return Ok(Plan {
                combo: Some(combo.clone()),
                targets,
            });
        }
        self.parse_target(&name)
            .map(|target| Plan {
                combo: None,
                targets: vec![target],
            })
            .ok_or_else(|| {
                LlmError::Config(format!(
                    "`{name}` is not a combo or a `provider/model` of a configured provider"
                ))
            })
    }
}

/// Where usage rows go, and where smart combos keep their sticky choices so
/// they survive restarts and are shared by every gateway of a task.
pub trait UsageSink: Send + Sync {
    fn record(&self, row: &UsageRow);

    fn record_switch(&self, _row: &SwitchRow) {}

    fn load_sticky(&self, _session: &str, _combo: &str) -> Option<Sticky> {
        None
    }

    fn save_sticky(&self, _session: &str, _combo: &str, _sticky: &Sticky) {}
}

/// Writes usage rows to the metadata database.
#[derive(Debug)]
pub struct StoreUsage(Mutex<Store>);

impl StoreUsage {
    /// # Errors
    /// Returns [`StoreError`] if the database cannot be opened.
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        Ok(Self(Mutex::new(Store::open(db_path)?)))
    }

    #[must_use]
    pub fn new(store: Store) -> Self {
        Self(Mutex::new(store))
    }
}

impl StoreUsage {
    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl UsageSink for StoreUsage {
    fn record(&self, row: &UsageRow) {
        if let Err(error) = self.store().record_usage(row) {
            tracing::warn!(target: "router", %error, "could not record usage");
        }
    }

    fn record_switch(&self, row: &SwitchRow) {
        if let Err(error) = self.store().record_switch(row) {
            tracing::warn!(target: "router", %error, "could not record a model switch");
        }
    }

    fn load_sticky(&self, session: &str, combo: &str) -> Option<Sticky> {
        let row = self
            .store()
            .get_sticky(session, combo)
            .map_err(
                |error| tracing::warn!(target: "router", %error, "could not read sticky model"),
            )
            .ok()??;
        Some(Sticky {
            model: row.model,
            window: serde_json::from_str(&row.window).unwrap_or_default(),
        })
    }

    fn save_sticky(&self, session: &str, combo: &str, sticky: &Sticky) {
        let row = StickyRow {
            session: session.into(),
            combo: combo.into(),
            model: sticky.model.clone(),
            window: serde_json::to_string(&sticky.window).unwrap_or_else(|_| "[]".into()),
        };
        if let Err(error) = self.store().set_sticky(&row) {
            tracing::warn!(target: "router", %error, "could not save sticky model");
        }
    }
}

/// Per-request switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    /// Role to resolve an empty model with (default: the gateway's role).
    pub role: Option<String>,
    /// `false` turns every token saver off for this request.
    pub token_saver: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            role: None,
            token_saver: true,
        }
    }
}

/// A smart combo's state during one request.
struct SmartRun {
    session: String,
    combo: String,
    /// Target labels in combo order.
    order: Vec<String>,
    threshold: f64,
    sticky: Option<Sticky>,
}

enum Outcome {
    Done(Completion),
    /// 2xx but nothing came back.
    Empty(Completion),
    Http {
        status: u16,
        error: LlmError,
    },
    /// The account has no key; try the next one without penalty.
    NoKey(String),
    /// Connection failure or a stream that broke before any content.
    Transport(LlmError),
    /// Broke after content reached the caller: cannot fall back.
    Fatal(LlmError),
}

struct Attempt<'a> {
    account: &'a Connection,
    target: &'a Target,
    role: &'a str,
    combo: Option<&'a str>,
}

/// The gateway. Cheap to clone.
#[derive(Clone)]
pub struct Gateway {
    http: reqwest::Client,
    config: Arc<GatewayConfig>,
    keys: Arc<dyn KeySource>,
    state: Arc<GatewayState>,
    usage: Option<Arc<dyn UsageSink>>,
    role: String,
    session: Option<String>,
    idle_timeout: Duration,
    probe_timeout: Duration,
}

impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field("connections", &self.config.connections.len())
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

impl Gateway {
    /// # Errors
    /// [`LlmError::Config`] if the HTTP client cannot be built.
    pub fn new(config: GatewayConfig, keys: Arc<dyn KeySource>) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .user_agent(concat!("next-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| LlmError::Config(error.to_string()))?;
        Ok(Self {
            http,
            config: Arc::new(config),
            keys,
            state: GatewayState::shared(),
            usage: None,
            role: "chat".into(),
            session: None,
            idle_timeout: Duration::from_secs(120),
            probe_timeout: Duration::from_secs(3),
        })
    }

    #[must_use]
    pub fn with_state(mut self, state: Arc<GatewayState>) -> Self {
        self.state = state;
        self
    }

    #[must_use]
    pub fn with_usage(mut self, usage: Arc<dyn UsageSink>) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Role used to resolve requests without a model (default `chat`).
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = role.into();
        self
    }

    /// Session (task id) smart combos keep their sticky model for.
    #[must_use]
    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }

    #[must_use]
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    #[must_use]
    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    #[must_use]
    pub fn state(&self) -> &GatewayState {
        &self.state
    }

    /// Streaming chat for an agent request.
    ///
    /// # Errors
    /// [`LlmError`] when every model and account failed.
    pub async fn chat(
        &self,
        request: &ChatRequest,
        options: &RequestOptions,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let body = serde_json::to_value(request).map_err(|e| LlmError::Protocol(e.to_string()))?;
        self.chat_value(body, options, on_delta).await
    }

    /// Streaming chat for a raw OpenAI chat body (images, files, tool choice…).
    ///
    /// # Errors
    /// [`LlmError`] when every model and account failed.
    #[allow(clippy::too_many_lines)] // One loop holding all fall-through rules reads best.
    pub async fn chat_value(
        &self,
        body: Value,
        options: &RequestOptions,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let role = options.role.clone().unwrap_or_else(|| self.role.clone());
        let requested = body.get("model").and_then(Value::as_str).unwrap_or("");
        let plan = self.config.resolve(requested, &role)?;
        let combo_name = plan.combo.as_ref().map(|c| c.name.clone());
        let mut targets = plan.targets;
        let mut smart_run = None;
        if let Some(combo) = &plan.combo {
            match combo
                .strategy
                .unwrap_or(self.config.settings.combo_strategy)
            {
                Strategy::Fallback => {}
                Strategy::RoundRobin => {
                    targets = rotate(
                        &targets,
                        self.state.next_use(&combo.name),
                        combo.sticky_limit,
                    );
                }
                Strategy::Smart => {
                    let session = self.session.clone().unwrap_or_default();
                    let sticky = self.load_sticky(&session, &combo.name);
                    let order: Vec<String> = targets.iter().map(Target::label).collect();
                    targets.rotate_left(smart::start_index(&order, sticky.as_ref()));
                    smart_run = Some(SmartRun {
                        order,
                        session,
                        combo: combo.name.clone(),
                        threshold: combo.confidence_threshold,
                        sticky,
                    });
                }
            }
        }
        let needed = required_caps(&body);
        reorder_by_caps(&mut targets, &needed, |t| {
            let provider = self
                .config
                .accounts(&t.provider)
                .first()
                .map_or_else(|| t.provider.clone(), |c| c.provider.clone());
            self.config.pricing.caps(&provider, &t.model)
        });

        let mut first_request_error: Option<LlmError> = None;
        let mut last_error: Option<LlmError> = None;
        let count = targets.len();
        for (index, target) in targets.iter().enumerate() {
            let last_model = index + 1 == count;
            let accounts = self.config.accounts(&target.provider);
            if accounts.is_empty() {
                last_error = Some(LlmError::Config(format!(
                    "no active connection for provider `{}`",
                    target.provider
                )));
                continue;
            }
            for account in accounts {
                if !self.state.available(&account.id, &target.model) {
                    last_error = Some(LlmError::Unavailable(format!(
                        "{} is cooling down after errors",
                        account.name
                    )));
                    continue;
                }
                let attempt = Attempt {
                    account,
                    target,
                    role: &role,
                    combo: combo_name.as_deref(),
                };
                match self.attempt(&attempt, &body, options, on_delta).await {
                    Outcome::Done(completion) => {
                        self.state.succeed(&account.id, &target.model);
                        self.observe_smart(smart_run.take(), target, &completion);
                        return Ok(completion);
                    }
                    Outcome::Empty(completion) => {
                        let final_reason = matches!(
                            completion.finish_reason.as_deref(),
                            Some("length" | "content_filter")
                        );
                        if last_model || final_reason {
                            self.observe_smart(smart_run.take(), target, &completion);
                            return Ok(completion);
                        }
                        last_error = Some(LlmError::Protocol(format!(
                            "{} returned an empty answer",
                            target.label()
                        )));
                        break;
                    }
                    Outcome::NoKey(name) => {
                        last_error = Some(LlmError::Config(format!(
                            "no API key stored for {name}; add one in Settings → Providers"
                        )));
                    }
                    Outcome::Http { status, error } => {
                        tracing::warn!(target: "router", provider = %target.label(), status, "attempt failed");
                        match status {
                            401..=403 => {
                                self.state.penalize(&account.id, None, &error.to_string());
                                last_error = Some(error);
                            }
                            429 => {
                                self.state.penalize(
                                    &account.id,
                                    Some(&target.model),
                                    &error.to_string(),
                                );
                                last_error = Some(error);
                            }
                            502..=504 => {
                                let delay = self.state.penalize(
                                    &account.id,
                                    Some(&target.model),
                                    &error.to_string(),
                                );
                                if delay <= SHORT_COOLDOWN && !last_model {
                                    tokio::time::sleep(delay).await;
                                }
                                last_error = Some(error);
                                break;
                            }
                            400..=499 => {
                                first_request_error.get_or_insert(error);
                                break;
                            }
                            _ => {
                                last_error = Some(error);
                                break;
                            }
                        }
                    }
                    Outcome::Transport(error) => {
                        last_error = Some(error);
                        break;
                    }
                    Outcome::Fatal(error) => return Err(error),
                }
            }
        }
        if let Some(error) = first_request_error {
            return Err(error);
        }
        match (count, last_error) {
            (1, Some(error)) => Err(error),
            (_, error) => Err(LlmError::Http {
                status: 503,
                body: format!(
                    "all {count} models failed{}",
                    error.map_or_else(String::new, |e| format!("; last error: {e}"))
                ),
            }),
        }
    }

    fn load_sticky(&self, session: &str, combo: &str) -> Option<Sticky> {
        self.state.sticky(session, combo).or_else(|| {
            let sticky = self.usage.as_ref()?.load_sticky(session, combo)?;
            self.state.set_sticky(session, combo, sticky.clone());
            Some(sticky)
        })
    }

    /// Score a smart combo's answer, move the sticky model if needed, persist
    /// it and record any switch.
    fn observe_smart(&self, run: Option<SmartRun>, answered: &Target, completion: &Completion) {
        let Some(SmartRun {
            session,
            combo,
            order,
            threshold,
            mut sticky,
        }) = run
        else {
            return;
        };
        let switch = smart::observe(
            &mut sticky,
            &order,
            &answered.label(),
            completion,
            threshold,
        );
        let Some(sticky) = sticky else { return };
        if self.state.sticky(&session, &combo).as_ref() == Some(&sticky) {
            return; // Nothing changed (the usual case): skip the write.
        }
        self.state.set_sticky(&session, &combo, sticky.clone());
        if let Some(sink) = &self.usage {
            sink.save_sticky(&session, &combo, &sticky);
            if let Some(switch) = &switch {
                sink.record_switch(&SwitchRow {
                    ts: None,
                    session: session.clone(),
                    combo: combo.clone(),
                    from_model: switch.from.clone(),
                    to_model: switch.to.clone(),
                    reason: switch.reason.clone(),
                    confidence: switch.confidence,
                });
            }
        }
        if let Some(switch) = switch {
            tracing::info!(target: "router", %combo, %session, from = %switch.from, to = %switch.to, reason = %switch.reason, "smart combo switched model");
        }
    }

    fn provider_body(
        account: &Connection,
        model: &str,
        openai: &Value,
        pricing: &Pricing,
    ) -> (Value, Format, gemini::NameMap) {
        let mut base = openai.clone();
        base["model"] = json!(model);
        base["stream"] = json!(true);
        match account.kind {
            ProviderKind::OpenAi => {
                base["stream_options"] = json!({"include_usage": true});
                (base, Format::OpenAi, gemini::NameMap::new())
            }
            ProviderKind::Anthropic => {
                let ceiling = pricing
                    .max_output(&account.provider, model)
                    .unwrap_or(anthropic::DEFAULT_MAX_OUTPUT);
                (
                    anthropic::request(&base, model, ceiling),
                    Format::Anthropic,
                    gemini::NameMap::new(),
                )
            }
            ProviderKind::Gemini => {
                let (body, names) = gemini::request(&base);
                (body, Format::Gemini, names)
            }
        }
    }

    fn build_request(
        &self,
        account: &Connection,
        model: &str,
        key: Option<&str>,
        body: &Value,
    ) -> reqwest::RequestBuilder {
        let builder = match account.kind {
            ProviderKind::OpenAi => {
                let builder = self.http.post(account.endpoint("chat/completions"));
                match key {
                    Some(key) => builder.bearer_auth(key),
                    None => builder,
                }
            }
            ProviderKind::Anthropic => self
                .http
                .post(account.endpoint("messages"))
                .header("x-api-key", key.unwrap_or_default())
                .header("anthropic-version", ANTHROPIC_VERSION),
            ProviderKind::Gemini => self
                .http
                .post(account.endpoint(&format!("models/{model}:streamGenerateContent?alt=sse")))
                .header("x-goog-api-key", key.unwrap_or_default()),
        };
        builder
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(body)
    }

    async fn attempt(
        &self,
        attempt: &Attempt<'_>,
        openai: &Value,
        options: &RequestOptions,
        on_delta: &mut dyn FnMut(&str),
    ) -> Outcome {
        let Attempt {
            account, target, ..
        } = *attempt;
        let started = Instant::now();
        let key = match self.keys.get(&account.id) {
            Ok(key) => key.filter(|k| !k.is_empty()),
            Err(error) => return Outcome::Transport(LlmError::Config(error.to_string())),
        };
        if account.needs_key && key.is_none() {
            return Outcome::NoKey(account.name.clone());
        }
        let (mut body, format, names) =
            Self::provider_body(account, &target.model, openai, &self.config.pricing);
        let report = savers::apply(
            &mut body,
            format,
            &self.config.settings.saver_config(options.token_saver),
        );
        let sent = self
            .build_request(account, &target.model, key.as_deref(), &body)
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                self.record(attempt, 0, Usage::default(), started, &report);
                return Outcome::Transport(LlmError::Unavailable(format!(
                    "{}: {}",
                    account.name,
                    describe(&error)
                )));
            }
        };
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            self.record(
                attempt,
                i64::from(status),
                Usage::default(),
                started,
                &report,
            );
            return Outcome::Http {
                status,
                error: classify_status(status, &text, &target.label()),
            };
        }
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        let (outcome, usage) = if is_json {
            Self::read_json(response, account.kind, &names, on_delta).await
        } else {
            self.read_stream(response, account.kind, names, on_delta)
                .await
        };
        self.record(attempt, i64::from(status), usage, started, &report);
        tracing::info!(
            target: "router",
            provider = %target.label(),
            latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            prompt_tokens = usage.prompt,
            completion_tokens = usage.completion,
            rtk_saved = report.bytes_before.saturating_sub(report.bytes_after),
            "gateway call finished"
        );
        outcome
    }

    async fn read_json(
        response: reqwest::Response,
        kind: ProviderKind,
        names: &gemini::NameMap,
        on_delta: &mut dyn FnMut(&str),
    ) -> (Outcome, Usage) {
        let body: Value = match response.json().await {
            Ok(body) => body,
            Err(error) => {
                return (
                    Outcome::Transport(LlmError::Protocol(error.to_string())),
                    Usage::default(),
                )
            }
        };
        let openai = match kind {
            ProviderKind::OpenAi => body,
            ProviderKind::Anthropic => anthropic::response(&body),
            ProviderKind::Gemini => gemini::response(&body, names),
        };
        let usage = openai
            .get("usage")
            .and_then(Usage::from_openai)
            .unwrap_or_default();
        let message = openai
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(Value::Null);
        // Feed the whole message through the accumulator as one delta.
        let mut acc = StreamAccumulator::new();
        let mut calls = message
            .get("tool_calls")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if let Some(list) = calls.as_array_mut() {
            for (index, call) in list.iter_mut().enumerate() {
                call["index"] = json!(index);
            }
        }
        let piece = json!({"choices": [{"delta": {"content": message.get("content").cloned().unwrap_or(Value::Null), "tool_calls": calls}, "finish_reason": openai.pointer("/choices/0/finish_reason").cloned().unwrap_or(Value::Null)}]});
        match acc.push(&piece) {
            Ok(Some(text)) => on_delta(&text),
            Ok(None) => {}
            Err(error) => return (Outcome::Transport(error), usage),
        }
        let completion = acc.finish();
        let empty = completion.content.is_empty() && completion.tool_calls.is_empty();
        (
            if empty {
                Outcome::Empty(completion)
            } else {
                Outcome::Done(completion)
            },
            usage,
        )
    }

    async fn read_stream(
        &self,
        response: reqwest::Response,
        kind: ProviderKind,
        names: gemini::NameMap,
        on_delta: &mut dyn FnMut(&str),
    ) -> (Outcome, Usage) {
        let mut decoder: Box<dyn StreamDecoder> = match kind {
            ProviderKind::OpenAi => Box::new(OpenAiDecoder),
            ProviderKind::Anthropic => Box::<anthropic::AnthropicDecoder>::default(),
            ProviderKind::Gemini => Box::new(gemini::GeminiDecoder::new(names)),
        };
        let mut events = response.bytes_stream().eventsource();
        let mut acc = StreamAccumulator::new();
        let mut usage = Usage::default();
        let mut committed = false;
        let fail = |error: LlmError, committed: bool| {
            if committed {
                Outcome::Fatal(error)
            } else {
                Outcome::Transport(error)
            }
        };
        loop {
            let Ok(next) = tokio::time::timeout(self.idle_timeout, events.next()).await else {
                let error = LlmError::Unavailable(format!(
                    "stream stalled for {} s",
                    self.idle_timeout.as_secs()
                ));
                return (fail(error, committed), usage);
            };
            let Some(event) = next else { break };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    return (
                        fail(LlmError::Protocol(error.to_string()), committed),
                        usage,
                    )
                }
            };
            let data = event.data.trim();
            if data == "[DONE]" {
                break;
            }
            if data.is_empty() {
                continue;
            }
            let parsed: Value = match serde_json::from_str(data) {
                Ok(value) => value,
                Err(error) => {
                    return (
                        fail(
                            LlmError::Protocol(format!("bad SSE chunk: {error}")),
                            committed,
                        ),
                        usage,
                    )
                }
            };
            let chunks = match decoder.decode(&event.event, &parsed) {
                Ok(chunks) => chunks,
                Err(error) => return (fail(error, committed), usage),
            };
            for piece in chunks {
                if let Some(found) = piece.get("usage").and_then(Usage::from_openai) {
                    usage = found;
                }
                match acc.push(&piece) {
                    Ok(Some(text)) => {
                        committed = true;
                        on_delta(&text);
                    }
                    Ok(None) => {}
                    Err(error) => return (fail(error, committed), usage),
                }
            }
        }
        let completion = acc.finish();
        let empty = completion.content.is_empty() && completion.tool_calls.is_empty();
        (
            if empty {
                Outcome::Empty(completion)
            } else {
                Outcome::Done(completion)
            },
            usage,
        )
    }

    fn record(
        &self,
        attempt: &Attempt<'_>,
        status: i64,
        usage: Usage,
        started: Instant,
        report: &SaverReport,
    ) {
        let Some(sink) = &self.usage else { return };
        let cost =
            self.config
                .pricing
                .cost(&attempt.account.provider, &attempt.target.model, &usage);
        sink.record(&UsageRow {
            ts: None,
            provider: attempt.account.provider.clone(),
            model: attempt.target.model.clone(),
            connection_id: Some(attempt.account.id.clone()),
            role: attempt.role.to_string(),
            combo: attempt.combo.map(str::to_string),
            endpoint: "chat".into(),
            status,
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            cached_tokens: usage.cached,
            cache_creation_tokens: usage.cache_creation,
            reasoning_tokens: usage.reasoning,
            cost: cost.usd,
            cost_known: cost.known,
            latency_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
            saver_bytes_before: i64::try_from(report.bytes_before).unwrap_or(i64::MAX),
            saver_bytes_after: i64::try_from(report.bytes_after).unwrap_or(i64::MAX),
        });
    }

    /// `GET` the provider's model list for one connection.
    ///
    /// # Errors
    /// [`LlmError`] for transport, auth or format failures.
    pub async fn list_models(&self, account: &Connection) -> Result<Vec<String>, LlmError> {
        let key = self
            .keys
            .get(&account.id)
            .map_err(|e| LlmError::Config(e.to_string()))?
            .filter(|k| !k.is_empty());
        if account.needs_key && key.is_none() {
            return Err(LlmError::Config(format!(
                "no API key stored for {}",
                account.name
            )));
        }
        let builder = self
            .http
            .get(account.endpoint("models"))
            .timeout(self.probe_timeout);
        let builder = match (account.kind, key.as_deref()) {
            (ProviderKind::OpenAi, Some(key)) => builder.bearer_auth(key),
            (ProviderKind::OpenAi, None) => builder,
            (ProviderKind::Anthropic, key) => builder
                .header("x-api-key", key.unwrap_or_default())
                .header("anthropic-version", ANTHROPIC_VERSION),
            (ProviderKind::Gemini, key) => {
                builder.header("x-goog-api-key", key.unwrap_or_default())
            }
        };
        let response = builder
            .send()
            .await
            .map_err(|e| LlmError::Unavailable(describe(&e)))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(classify_status(status, &text, "models"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| LlmError::Protocol(e.to_string()))?;
        let ids: Vec<String> = match account.kind {
            ProviderKind::Gemini => body
                .get("models")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| m.get("name").and_then(Value::as_str))
                .map(|n| n.trim_start_matches("models/").to_string())
                .collect(),
            _ => body
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| LlmError::Protocol("models response has no `data` array".into()))?
                .iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
        };
        Ok(ids)
    }

    /// [`Self::list_models`] from synchronous code (not from an async worker).
    ///
    /// # Errors
    /// See [`Self::list_models`].
    pub fn list_models_blocking(&self, account: &Connection) -> Result<Vec<String>, LlmError> {
        super::block_on(self.list_models(account))
    }

    /// Health of the provider behind the gateway's role (the status-bar dot).
    pub async fn health(&self) -> HealthReport {
        let started = Instant::now();
        let plan = match self.config.resolve("", &self.role) {
            Ok(plan) => plan,
            Err(LlmError::Config(detail)) => return HealthReport::new(HealthStatus::Down, detail),
            Err(other) => return HealthReport::new(HealthStatus::Down, other.to_string()),
        };
        let Some((target, account)) = plan.targets.iter().find_map(|t| {
            self.config
                .accounts(&t.provider)
                .first()
                .map(|a| (t.clone(), (*a).clone()))
        }) else {
            return HealthReport::new(
                HealthStatus::Down,
                "no active connection for the default model",
            );
        };
        let label = format!("{} · {}", account.name, target.model);
        let result = self.list_models(&account).await;
        let latency = started.elapsed();
        match result {
            Ok(_) if latency > Duration::from_millis(1_500) => HealthReport::new(
                HealthStatus::Degraded,
                format!("{label}: slow response ({} ms)", latency.as_millis()),
            ),
            Ok(_) => HealthReport::new(HealthStatus::Ok, label),
            Err(LlmError::Unauthorized) => HealthReport::new(
                HealthStatus::Degraded,
                format!("{label}: unauthorized (check the API key)"),
            ),
            Err(LlmError::Unavailable(detail)) => {
                HealthReport::new(HealthStatus::Down, format!("{label}: {detail}"))
            }
            Err(other) => HealthReport::new(HealthStatus::Degraded, format!("{label}: {other}")),
        }
        .with_latency(latency)
    }
}

impl CompletionClient for Gateway {
    fn complete(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        super::block_on(self.chat(request, &RequestOptions::default(), on_delta))
    }
}

impl HealthProbe for Gateway {
    fn probe(&self) -> ProbeFuture<'_> {
        Box::pin(self.health())
    }
}

#[cfg(test)]
mod tests;
