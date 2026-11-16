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
//! Before the attempts, in order: budget caps ([`super::budget`]), the combo
//! strategy, capability reorder, the context overflow switch
//! ([`super::overflow`]), circuit-breaker health ([`super::breaker`]),
//! the per-role reasoning setting ([`super::reasoning`]) and the response
//! cache ([`super::cache`]). Within a model, accounts are ranked by
//! prompt-cache affinity ([`super::affinity`]) when there are several.
//!
//! A `smart` combo starts at the session's sticky model and updates it after
//! the answer (see [`super::smart`]); the session is the task id given by
//! [`Gateway::with_session`]. Calls without a
//! session share one sticky model per combo. Concurrent calls of one session
//! may overwrite each other's quality window (last write wins).

use super::budget::{Budget, BudgetAction, BudgetStatus, Level, Period};
use super::cache::{self, Entry, ResponseCacheSettings};
use super::client::{classify_status, describe, parse_models};
use super::combo::{
    reorder_by_caps, required_caps, rotate, sort_known, weighted_order, Combo, GatewayState,
    Reservation, Strategy,
};
use super::pricing::Pricing;
use super::providers::{Connection, KeySource, ProviderKind, LEGACY_ROUTER_ID};
use super::reasoning::{self, ReasoningSetting};
use super::savers::{self, prompts::Level as SaverLevel, Format, SaverConfig, SaverReport};
use super::settings::RouterSettings;
use super::smart::{self, Sticky};
use super::translate::{anthropic, gemini, OpenAiDecoder, StreamDecoder, Usage};
use super::{affinity, breaker, combo, overflow};
use crate::llm::{ChatRequest, Completion, CompletionClient, LlmError, StreamAccumulator};
use crate::store::{CacheHitRow, StickyRow, Store, StoreError, SwitchRow, UsageRow};
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
/// Output ceiling assumed when the price table has no `max_output` for a model.
const FALLBACK_OUTPUT_TOKENS: u32 = 16_384;
/// HTTP status returned when a budget refuses a request.
const BUDGET_BLOCKED_STATUS: u16 = 402;

/// User-editable gateway settings.
#[allow(clippy::struct_excessive_bools)] // Independent on/off switches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GatewaySettings {
    /// `provider/model` or combo name used when a role has no assignment.
    pub default_model: Option<String>,
    /// Global token-saver switch.
    pub token_saver: bool,
    pub caveman: Option<SaverLevel>,
    pub ponytail: Option<SaverLevel>,
    pub rtk: bool,
    pub cache_control: bool,
    /// Strategy for combos that do not set their own.
    pub combo_strategy: Strategy,
    /// Role (`chat`, `design`, `summary`, `review`) → model or combo.
    pub roles: BTreeMap<String, String>,
    /// Keep a conversation on one account (prompt-cache affinity).
    pub cache_affinity: bool,
    pub response_cache: ResponseCacheSettings,
    /// Role → reasoning effort or budget.
    pub reasoning: BTreeMap<String, ReasoningSetting>,
    pub budgets: Vec<Budget>,
    pub budget_action: BudgetAction,
    /// While any budget applies, refuse models without a known price (their
    /// spend would count as $0).
    pub budget_refuse_unpriced: bool,
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
            cache_affinity: true,
            response_cache: ResponseCacheSettings::default(),
            reasoning: BTreeMap::new(),
            budgets: Vec::new(),
            budget_action: BudgetAction::Block,
            budget_refuse_unpriced: true,
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
        clean
            .reasoning
            .retain(|role, setting| ROLES.contains(&role.as_str()) && setting.to_body().is_some());
        clean.budgets.retain(|b| b.limit_usd > 0.0);
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

    /// A cached answer (serialized [`Entry`]) that has not expired.
    fn cache_get(&self, _key: &str) -> Option<String> {
        None
    }

    fn cache_put(
        &self,
        _key: &str,
        _target: &Target,
        _entry: &str,
        _settings: &ResponseCacheSettings,
    ) {
    }

    fn record_cache_hit(&self, _row: &CacheHitRow) {}

    /// Spend today (`monthly = false`) or this month, for a role or overall.
    /// `Ok(None)`: this sink does not track spend (budgets are not enforced).
    ///
    /// # Errors
    /// The spend could not be read; budgets then fail closed.
    fn spend(&self, _role: Option<&str>, _monthly: bool) -> Result<Option<f64>, String> {
        Ok(None)
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
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

    fn cache_get(&self, key: &str) -> Option<String> {
        self.store()
            .cache_get(key, unix_now())
            .map_err(|error| tracing::warn!(target: "router", %error, "response cache read failed"))
            .ok()?
    }

    fn cache_put(&self, key: &str, target: &Target, entry: &str, settings: &ResponseCacheSettings) {
        let result = self.store().cache_put(
            key,
            &target.provider,
            &target.model,
            entry,
            unix_now(),
            i64::from(settings.ttl_secs),
            i64::from(settings.max_entries),
        );
        if let Err(error) = result {
            tracing::warn!(target: "router", %error, "response cache write failed");
        }
    }

    fn record_cache_hit(&self, row: &CacheHitRow) {
        if let Err(error) = self.store().record_cache_hit(row) {
            tracing::warn!(target: "router", %error, "could not record a cache hit");
        }
    }

    fn spend(&self, role: Option<&str>, monthly: bool) -> Result<Option<f64>, String> {
        self.store()
            .spend(role, monthly)
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

/// Per-request switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    /// Role to resolve an empty model with (default: the gateway's role).
    pub role: Option<String>,
    /// `false` turns every token saver off for this request.
    pub token_saver: bool,
    /// `false` bypasses the response cache for this request.
    pub cache: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            role: None,
            token_saver: true,
            cache: true,
        }
    }
}

/// What to store in the response cache after a complete answer.
struct CacheStore<'a> {
    key: String,
    deltas: &'a [String],
}

/// A successful provider answer.
struct Answer {
    completion: Completion,
    /// OpenRouter-style `reasoning_details` seen in the answer.
    details: Vec<Value>,
    usage: Usage,
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
    Done(Answer),
    /// 2xx but nothing came back.
    Empty(Answer),
    Http {
        status: u16,
        error: LlmError,
    },
    /// The account has no key; try the next one without penalty.
    NoKey(String),
    /// The local keychain could not be read: a config problem on this
    /// machine, not a provider failure, so the breaker is left alone.
    KeyUnavailable(LlmError),
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

/// Done or Empty, by whether anything came back.
fn conclude(completion: Completion, details: Vec<Value>, usage: Usage) -> Outcome {
    let empty = completion.content.is_empty() && completion.tool_calls.is_empty();
    let answer = Answer {
        completion,
        details,
        usage,
    };
    if empty {
        Outcome::Empty(answer)
    } else {
        Outcome::Done(answer)
    }
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
    /// [`LlmError`] when a budget blocks the request or every model and
    /// account failed.
    #[allow(clippy::too_many_lines)] // One loop holding all fall-through rules reads best.
    pub async fn chat_value(
        &self,
        mut body: Value,
        options: &RequestOptions,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let role = options.role.clone().unwrap_or_else(|| self.role.clone());
        let requested = body.get("model").and_then(Value::as_str).unwrap_or("");
        let plan = self.config.resolve(requested, &role)?;
        let combo_name = plan.combo.as_ref().map(|c| c.name.clone());
        let mut targets = plan.targets;
        // Held until this request ends; released when dropped.
        let _reservation = self.enforce_budgets(&role, &body, &mut targets)?;
        let mut smart_run = self.order_by_strategy(plan.combo.as_ref(), &mut targets);
        let needed = required_caps(&body);
        reorder_by_caps(&mut targets, &needed, |t| {
            self.config.pricing.caps(&self.price_provider(t), &t.model)
        });
        let tokens = overflow::estimate_tokens(&body);
        if overflow::prefer_fitting(&mut targets, tokens, |t| {
            self.config
                .pricing
                .context_length(&self.price_provider(t), &t.model)
        }) {
            tracing::info!(target: "router", tokens, "prompt too large for some models; trying ones that fit first");
        }
        // Providers whose breaker is open go last (they would be refused);
        // half-open ones keep their place so the strategy's order holds.
        targets.sort_by_cached_key(|t| self.provider_health(&t.provider) <= 0.0);
        self.prepare_body(&mut body, &role);

        let caching = options.cache
            && self.usage.is_some()
            && cache::eligible(&body, &self.config.settings.response_cache);
        if caching {
            if let Some(answer) = self.cached_answer(&body, &targets, &role, options, on_delta) {
                return Ok(answer);
            }
        }
        let affinity_key = self
            .config
            .settings
            .cache_affinity
            .then(|| affinity::key(&body))
            .flatten();
        let mut captured: Vec<String> = Vec::new();
        let mut tee = |delta: &str| {
            if caching {
                captured.push(delta.to_string());
            }
            on_delta(delta);
        };

        let mut first_request_error: Option<LlmError> = None;
        let mut last_error: Option<LlmError> = None;
        let count = targets.len();
        for (index, target) in targets.iter().enumerate() {
            let last_model = index + 1 == count;
            let mut accounts = self.config.accounts(&target.provider);
            if accounts.is_empty() {
                last_error = Some(LlmError::Config(format!(
                    "no active connection for provider `{}`",
                    target.provider
                )));
                continue;
            }
            if let Some(key) = &affinity_key {
                // Only among accounts of equal priority: the user's order wins.
                for group in accounts.chunk_by_mut(|a, b| a.priority == b.priority) {
                    affinity::rank(
                        group,
                        key,
                        |a| a.id.clone(),
                        |a| {
                            let usable = self.state.available(&a.id, &target.model);
                            self.state.breakers.state(&a.provider).score()
                                * f64::from(u8::from(usable))
                        },
                    );
                }
            }
            for account in accounts {
                if !self.state.available(&account.id, &target.model) {
                    last_error = Some(LlmError::Unavailable(format!(
                        "{} is cooling down after errors",
                        account.name
                    )));
                    continue;
                }
                if self.state.breakers.permit(&account.provider) == breaker::Permit::Refused {
                    last_error = Some(LlmError::Unavailable(format!(
                        "{} is failing repeatedly; paused (circuit open)",
                        account.provider
                    )));
                    continue;
                }
                let attempt = Attempt {
                    account,
                    target,
                    role: &role,
                    combo: combo_name.as_deref(),
                };
                let outcome = self.attempt(&attempt, &body, options, &mut tee).await;
                self.report_breaker(&account.provider, &outcome);
                match outcome {
                    Outcome::Done(answer) => {
                        self.state.succeed(&account.id, &target.model);
                        let store = (caching
                            && cache::storable(answer.completion.finish_reason.as_deref()))
                        .then(|| CacheStore {
                            key: self.cache_key(&body, account, &target.model, options),
                            deltas: &captured,
                        });
                        return Ok(self.finish(
                            answer,
                            target,
                            combo_name.as_deref(),
                            smart_run.take(),
                            store,
                        ));
                    }
                    Outcome::Empty(answer) => {
                        let final_reason = matches!(
                            answer.completion.finish_reason.as_deref(),
                            Some("length" | "content_filter")
                        );
                        if last_model || final_reason {
                            return Ok(self.finish(
                                answer,
                                target,
                                combo_name.as_deref(),
                                smart_run.take(),
                                None,
                            ));
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
                    Outcome::KeyUnavailable(error) => {
                        tracing::warn!(target: "router", provider = %account.provider, kind = "keychain", error = %error, "API key could not be read");
                        last_error = Some(error);
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

    /// The provider name prices are filed under for a target (a connection
    /// id or name in the target resolves to its provider).
    fn price_provider(&self, target: &Target) -> String {
        self.config
            .accounts(&target.provider)
            .first()
            .map_or_else(|| target.provider.clone(), |c| c.provider.clone())
    }

    /// Breaker score of the provider behind a target address (1 = healthy).
    fn provider_health(&self, address: &str) -> f64 {
        self.config
            .accounts(address)
            .iter()
            .map(|a| self.state.breakers.state(&a.provider).score())
            .fold(0.0, f64::max)
    }

    fn report_breaker(&self, provider: &str, outcome: &Outcome) {
        let breakers = &self.state.breakers;
        match outcome {
            Outcome::Http { status, .. } if *status >= 500 => {
                breakers.failure(provider);
            }
            Outcome::Http { status: 429, .. } | Outcome::NoKey(_) | Outcome::KeyUnavailable(_) => {
                breakers.release(provider);
            }
            // Any answer, even a request-scoped 4xx, shows the provider is up.
            Outcome::Done(_) | Outcome::Empty(_) | Outcome::Http { .. } => {
                breakers.success(provider);
            }
            Outcome::Transport(_) | Outcome::Fatal(_) => {
                breakers.failure(provider);
            }
        }
    }

    /// Apply the combo's strategy to `targets`; a smart combo also returns
    /// its per-request state.
    fn order_by_strategy(
        &self,
        combo: Option<&Combo>,
        targets: &mut Vec<Target>,
    ) -> Option<SmartRun> {
        let combo = combo?;
        match combo
            .strategy
            .unwrap_or(self.config.settings.combo_strategy)
        {
            Strategy::Fallback => {}
            Strategy::RoundRobin => {
                *targets = rotate(
                    targets,
                    self.state.next_use(&combo.name),
                    combo.sticky_limit,
                );
            }
            Strategy::Weighted => {
                // Weights follow the combo's model list; resolving may have
                // dropped models without a provider, so match by model.
                let weights: Vec<u32> = targets
                    .iter()
                    .map(|t| {
                        combo
                            .models
                            .iter()
                            .position(|m| self.config.parse_target(m).as_ref() == Some(t))
                            .and_then(|i| combo.weights.get(i).copied())
                            .unwrap_or(1)
                    })
                    .collect();
                *targets = weighted_order(targets, &weights, || self.state.draw());
            }
            Strategy::CostOptimized => sort_known(targets, |t| {
                self.config
                    .pricing
                    .lookup(&self.price_provider(t), &t.model)
                    .and_then(|p| p.input)
            }),
            Strategy::ContextOptimized => sort_known(targets, |t| {
                self.config
                    .pricing
                    .context_length(&self.price_provider(t), &t.model)
                    .map(std::cmp::Reverse)
            }),
            Strategy::Lkgp => {
                if let Some(good) = self.state.last_good(&combo.name) {
                    if let Some(at) = targets.iter().position(|t| t.label() == good) {
                        let first = targets.remove(at);
                        targets.insert(0, first);
                    }
                }
            }
            Strategy::Smart => {
                let session = self.session.clone().unwrap_or_default();
                let sticky = self.load_sticky(&session, &combo.name);
                let order: Vec<String> = targets.iter().map(Target::label).collect();
                targets.rotate_left(smart::start_index(&order, sticky.as_ref()));
                return Some(SmartRun {
                    order,
                    session,
                    combo: combo.name.clone(),
                    threshold: combo.confidence_threshold,
                    sticky,
                });
            }
        }
        None
    }

    /// Every configured budget with its current spend, in-flight
    /// reservations included (empty without a usage store or when the spend
    /// cannot be read).
    #[must_use]
    pub fn budget_status(&self) -> Vec<BudgetStatus> {
        self.budgets_for(None).unwrap_or_default()
    }

    /// Budgets that apply to `role` (all with `None`), with their spend.
    fn budgets_for(&self, role: Option<&str>) -> Result<Vec<BudgetStatus>, String> {
        let Some(sink) = &self.usage else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for budget in self
            .config
            .settings
            .budgets
            .iter()
            .filter(|b| b.limit_usd > 0.0 && role.is_none_or(|r| b.applies_to(r)))
        {
            let Some(spent) =
                sink.spend(budget.role.as_deref(), budget.period == Period::Monthly)?
            else {
                continue;
            };
            let reserved = self.state.reserved(budget.role.as_deref());
            out.push(BudgetStatus::new(budget.clone(), spent + reserved));
        }
        Ok(out)
    }

    /// Worst-case estimated cost of sending `body` to any of `targets`: the
    /// prompt estimate plus the requested output or, when none is given, the
    /// output ceiling the request will be sent with (reasoning included).
    fn estimate_cost(&self, body: &Value, targets: &[Target]) -> f64 {
        let requested = body
            .get("max_completion_tokens")
            .or_else(|| body.get("max_tokens"))
            .and_then(Value::as_i64);
        let prompt = i64::try_from(overflow::estimate_tokens(body))
            .unwrap_or(i64::MAX)
            .saturating_sub(requested.unwrap_or(0))
            .max(0);
        targets
            .iter()
            .map(|t| {
                let provider = self.price_provider(t);
                let output = requested.unwrap_or_else(|| {
                    i64::from(
                        self.config
                            .pricing
                            .max_output(&provider, &t.model)
                            .unwrap_or(FALLBACK_OUTPUT_TOKENS),
                    )
                });
                let usage = Usage {
                    prompt,
                    completion: output,
                    ..Usage::default()
                };
                self.config.pricing.cost(&provider, &t.model, &usage).usd
            })
            .fold(0.0, f64::max)
    }

    /// Budget caps for `role`. Warns at 80 %. Fails closed when the spend
    /// cannot be read. While a cap applies, unpriced models are refused (if
    /// set) and the request's estimated cost is reserved, so concurrent
    /// requests cannot all slip under the cap. At 100 % (spend + reservations
    /// + this estimate) it blocks, or keeps only the cheapest priced target.
    fn enforce_budgets(
        &self,
        role: &str,
        body: &Value,
        targets: &mut Vec<Target>,
    ) -> Result<Option<Reservation>, LlmError> {
        // Check and reserve under one lock: two requests must not both pass
        // on the same remaining budget.
        let _gate = self.state.budget_gate();
        let statuses = self.budgets_for(Some(role)).map_err(|error| {
            tracing::warn!(target: "router", %error, "could not read spend; budgets fail closed");
            LlmError::Http {
                status: BUDGET_BLOCKED_STATUS,
                body: format!("Budgets are set but today's spend could not be read ({error}); the request was not sent."),
            }
        })?;
        if statuses.is_empty() {
            return Ok(None);
        }
        let priced = |t: &Target| {
            self.config
                .pricing
                .cost(&self.price_provider(t), &t.model, &Usage::default())
                .known
        };
        if self.config.settings.budget_refuse_unpriced {
            targets.retain(|t| priced(t));
            if targets.is_empty() {
                return Err(LlmError::Http {
                    status: BUDGET_BLOCKED_STATUS,
                    body: "A budget is set, and none of these models has a known price, so their spend could not be counted. Add a price in pricing.json, refresh the model catalog, or allow unpriced models in Settings → Providers → Budgets.".into(),
                });
            }
        }
        for status in statuses.iter().filter(|s| s.level == Level::Warn) {
            tracing::warn!(target: "router", budget = %status.budget.label(), spent = status.spent, "budget above {:.0}%", super::budget::WARN_AT * 100.0);
        }
        let estimate = self.estimate_cost(body, targets);
        let over = statuses
            .iter()
            .find(|s| s.spent + estimate >= s.budget.limit_usd);
        if let Some(over) = over {
            let blocked = || LlmError::Http {
                status: BUDGET_BLOCKED_STATUS,
                body: over.message(),
            };
            if self.config.settings.budget_action == BudgetAction::Block {
                return Err(blocked());
            }
            let price = |t: &Target| {
                self.config
                    .pricing
                    .lookup(&self.price_provider(t), &t.model)
                    .and_then(|p| Some(p.input? + p.output?))
            };
            let cheapest = targets
                .iter()
                .filter_map(|t| price(t).map(|p| (p, t.clone())))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, t)| t)
                .ok_or_else(blocked)?;
            tracing::warn!(target: "router", budget = %over.budget.label(), model = %cheapest.label(), "budget reached; using the cheapest model");
            *targets = vec![cheapest];
        }
        let estimate = self.estimate_cost(body, targets);
        Ok(Some(self.state.reserve(role, estimate)))
    }

    /// Add the role's reasoning setting and restore remembered reasoning
    /// blocks on earlier tool-call turns.
    fn prepare_body(&self, body: &mut Value, role: &str) {
        if body.get("reasoning").is_none() {
            let setting = self
                .config
                .settings
                .reasoning
                .get(role)
                .and_then(|r| r.to_body());
            if let Some(reasoning) = setting {
                body["reasoning"] = reasoning;
            }
        }
        self.state.restore_reasoning(body);
    }

    /// Settings that reshape the request after the cache key is taken
    /// (token savers): part of the key so a change cannot serve stale shapes.
    fn cache_salt(&self, options: &RequestOptions) -> Value {
        let s = &self.config.settings;
        json!({
            "savers": s.token_saver && options.token_saver,
            "caveman": s.caveman.map(|l| format!("{l:?}")),
            "ponytail": s.ponytail.map(|l| format!("{l:?}")),
            "rtk": s.rtk,
            "cache_control": s.cache_control,
        })
    }

    /// Cache key for `body` on one account: its id and base URL are part of
    /// the key, so different endpoints never share answers.
    fn cache_key(
        &self,
        body: &Value,
        account: &Connection,
        model: &str,
        options: &RequestOptions,
    ) -> String {
        let endpoint = format!("{}|{}", account.id, account.base_url);
        cache::key(body, &endpoint, model, &self.cache_salt(options))
    }

    /// Serve `body` from the response cache, replaying the stored deltas.
    fn cached_answer(
        &self,
        body: &Value,
        targets: &[Target],
        role: &str,
        options: &RequestOptions,
        on_delta: &mut dyn FnMut(&str),
    ) -> Option<Completion> {
        let sink = self.usage.as_ref()?;
        // Only the account each target would be sent to first: a provider
        // name can cover several endpoints, which must not share answers.
        let (target, entry) = targets.iter().find_map(|t| {
            let account = *self.config.accounts(&t.provider).first()?;
            let raw = sink.cache_get(&self.cache_key(body, account, &t.model, options))?;
            serde_json::from_str::<Entry>(&raw)
                .map_err(
                    |error| tracing::warn!(target: "router", %error, "ignoring a bad cache entry"),
                )
                .ok()
                .map(|entry| (t, entry))
        })?;
        for delta in &entry.deltas {
            on_delta(delta);
        }
        let completion = entry.completion();
        self.remember_reasoning(&completion, entry.reasoning_details.clone());
        sink.record_cache_hit(&CacheHitRow {
            provider: self.price_provider(target),
            model: target.model.clone(),
            role: role.to_string(),
            saved_tokens: entry.prompt_tokens + entry.completion_tokens,
            saved_cost: entry.cost,
        });
        tracing::info!(target: "router", provider = %target.label(), "served from the response cache");
        Some(completion)
    }

    /// Keep an answer's reasoning blocks for its tool calls.
    fn remember_reasoning(&self, completion: &Completion, details: Vec<Value>) {
        if details.is_empty() || completion.tool_calls.is_empty() {
            return;
        }
        let keys: Vec<String> = completion
            .tool_calls
            .iter()
            .filter_map(|c| serde_json::to_value(c).ok())
            .filter_map(|c| combo::tool_call_key(&c))
            .collect();
        self.state.remember_reasoning(&keys, &Value::Array(details));
    }

    /// Book-keeping after a provider answered: usage for the agent, routing
    /// memory, reasoning blocks, the response cache and smart-combo state.
    fn finish(
        &self,
        answer: Answer,
        target: &Target,
        combo: Option<&str>,
        smart_run: Option<SmartRun>,
        store: Option<CacheStore<'_>>,
    ) -> Completion {
        let Answer {
            mut completion,
            details,
            usage,
        } = answer;
        // Hand the provider's counts to the agent engine (context compaction
        // anchors its size estimate on them). OpenAI-style `prompt` includes
        // cached and cache-write tokens; `TokenUsage::input` excludes them.
        if completion.usage.is_none() && usage != Usage::default() {
            let count = |n: i64| u64::try_from(n).unwrap_or(0);
            completion.usage = Some(crate::llm::TokenUsage {
                input: count(usage.prompt - usage.cached - usage.cache_creation),
                cache_read: count(usage.cached),
                cache_write: count(usage.cache_creation),
                output: count(usage.completion),
                reasoning: count(usage.reasoning),
            });
        }
        if let Some(combo) = combo {
            self.state.set_last_good(combo, &target.label());
        }
        if let (Some(store), Some(sink)) = (store, &self.usage) {
            let provider = self.price_provider(target);
            let entry = Entry {
                deltas: store.deltas.to_vec(),
                content: completion.content.clone(),
                tool_calls: completion.tool_calls.clone(),
                finish_reason: completion.finish_reason.clone(),
                reasoning_details: details.clone(),
                prompt_tokens: usage.prompt,
                completion_tokens: usage.completion,
                cost: self
                    .config
                    .pricing
                    .cost(&provider, &target.model, &usage)
                    .usd,
            };
            if let Ok(text) = serde_json::to_string(&entry) {
                sink.cache_put(
                    &store.key,
                    target,
                    &text,
                    &self.config.settings.response_cache,
                );
            }
        }
        self.remember_reasoning(&completion, details);
        self.observe_smart(smart_run, target, &completion);
        completion
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
        let max_output = pricing.max_output(&account.provider, model);
        match account.kind {
            ProviderKind::OpenAi => {
                base["stream_options"] = json!({"include_usage": true});
                reasoning::apply_openai(
                    &mut base,
                    &account.provider,
                    max_output.unwrap_or(FALLBACK_OUTPUT_TOKENS),
                );
                (base, Format::OpenAi, gemini::NameMap::new())
            }
            ProviderKind::Anthropic => {
                let ceiling = max_output.unwrap_or(anthropic::DEFAULT_MAX_OUTPUT);
                let mut body = anthropic::request(&base, model, ceiling);
                reasoning::apply_anthropic(&mut body, &base, ceiling);
                (body, Format::Anthropic, gemini::NameMap::new())
            }
            ProviderKind::Gemini => {
                let (mut body, names) = gemini::request(&base);
                reasoning::apply_gemini(&mut body, &base, model, max_output.unwrap_or(65_536));
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
            Err(error) => {
                return Outcome::KeyUnavailable(LlmError::Config(format!(
                    "could not read the API key for {}: {error}",
                    account.name
                )));
            }
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
        let details = message
            .get("reasoning_details")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        (conclude(acc.finish(), details, usage), usage)
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
        let mut details = Vec::new();
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
                if let Some(found) = piece
                    .pointer("/choices/0/delta/reasoning_details")
                    .and_then(Value::as_array)
                {
                    reasoning::merge_details(&mut details, found);
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
        (conclude(acc.finish(), details, usage), usage)
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
            _ => parse_models(&body)?,
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
        match self.state.breakers.state(&account.provider) {
            breaker::BreakerState::Open => {
                return HealthReport::new(
                    HealthStatus::Down,
                    format!("{label}: paused after repeated failures (circuit open)"),
                )
            }
            breaker::BreakerState::HalfOpen => {
                return HealthReport::new(
                    HealthStatus::Degraded,
                    format!("{label}: recovering (circuit half-open)"),
                )
            }
            breaker::BreakerState::Closed => {}
        }
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
