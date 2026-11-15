//! Chat channels in the desktop app (Settings → Channels): starts each
//! enabled channel with its bot token from the OS keychain, supervises it,
//! and runs inbound messages as ordinary tasks through
//! [`AppService::task_start_with`] (broker + permission engine, manual or
//! plan only, channel tool profile, approvals on the desktop or overlay with
//! deny on timeout). Rules and design: `docs/architecture/Channels.md`.
//!
//! Scheduler hook: [`ChannelService::deliver`] sends a message to a paired
//! chat. The memory-skills cron calls it for jobs whose target is a
//! channel (see the doc).

use crate::service::{AppService, Sink, TaskRequest};
use crate::tasks::ChannelLaunch;
use crate::wire::StreamMessage;
use next_agent_channels::hub::{Delivery, HubEvent};
use next_agent_channels::secret::{read_token, token_entry};
use next_agent_channels::telegram::{self, api::DEFAULT_BASE_URL, TelegramChannel};
use next_agent_channels::{
    channel_profile, supervise, AllowEntry, Backoff, BotIdentity, BoxFuture, ChannelConfig,
    ChannelJob, ChannelState, Health, HealthState, Hub, PairingRequest, StopToken, StoreState,
    Supervised, TaskOutcome, TaskProgress, TaskRunner,
};
use next_agent_core::router::SecretStore;
use next_agent_core::store::ChannelRow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Pushes an event to the UI (`channel-health`, `task-started`, …).
pub type UiEmit = Arc<dyn Fn(&'static str, Value) + Send + Sync>;
/// Keychain entry for a channel id (swapped for memory in tests).
pub type Secrets = Arc<dyn Fn(&str) -> Arc<dyn SecretStore> + Send + Sync>;

/// A channel as the Settings page sees it. Never carries the token.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    pub config: ChannelConfig,
    pub has_token: bool,
    pub health: Option<Health>,
}

/// Save request from Settings. `token` is only sent when the owner pastes
/// a new one; it goes straight to the keychain.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInput {
    pub config: ChannelConfig,
    #[serde(default)]
    pub token: Option<String>,
}

impl std::fmt::Debug for ChannelInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelInput")
            .field("config", &self.config)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

struct Running {
    supervised: Supervised,
    pump: JoinHandle<()>,
    watcher: JoinHandle<()>,
}

/// See the module docs.
pub struct ChannelService {
    app: Arc<AppService>,
    hub: Arc<Hub>,
    state: Arc<StoreState>,
    secrets: Secrets,
    base_url: String,
    emit: UiEmit,
    running: tokio::sync::Mutex<HashMap<String, Running>>,
    health: Arc<Mutex<HashMap<String, Health>>>,
}

fn health_now(state: HealthState, detail: &str) -> Health {
    Health::new(state, detail, 0)
}

fn new_id() -> String {
    format!(
        "ch_{}",
        next_agent_channels::pairing::random_code().to_lowercase()
    )
}

impl ChannelService {
    /// Production wiring: keychain secrets, the real Bot API.
    ///
    /// # Errors
    /// The database cannot be opened.
    pub fn new(app: Arc<AppService>, emit: UiEmit) -> Result<Arc<Self>, String> {
        Self::with(
            app,
            emit,
            Arc::new(|id: &str| Arc::new(token_entry(id)) as Arc<dyn SecretStore>),
            DEFAULT_BASE_URL,
        )
    }

    /// Injected secrets and API base (tests).
    ///
    /// # Errors
    /// The database cannot be opened.
    pub fn with(
        app: Arc<AppService>,
        emit: UiEmit,
        secrets: Secrets,
        base_url: &str,
    ) -> Result<Arc<Self>, String> {
        let state = Arc::new(StoreState::open(app.db_path())?);
        let runner = Arc::new(DesktopRunner {
            app: Arc::clone(&app),
            emit: Arc::clone(&emit),
        });
        let hub = Hub::new(state.clone() as Arc<dyn ChannelState>, runner);
        let notify = Arc::clone(&emit);
        hub.set_notify(move |event| match event {
            HubEvent::PairingRequested { channel_id } => {
                notify("channel-pairing", json!({ "channelId": channel_id }));
            }
            HubEvent::Activity { channel_id } => {
                notify("channel-activity", json!({ "channelId": channel_id }));
            }
        });
        Ok(Arc::new(Self {
            app,
            hub,
            state,
            secrets,
            base_url: base_url.to_string(),
            emit,
            running: tokio::sync::Mutex::default(),
            health: Arc::default(),
        }))
    }

    fn configs(&self) -> Result<Vec<ChannelConfig>, String> {
        let rows = self
            .app
            .store()?
            .channel_rows()
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let mut config: ChannelConfig = serde_json::from_str(&row.config).ok()?;
                config.id = row.id;
                Some(config)
            })
            .collect())
    }

    fn config(&self, id: &str) -> Result<ChannelConfig, String> {
        self.configs()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("unknown channel {id}"))
    }

    fn set_health(&self, id: &str, health: &Health) {
        self.health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_string(), health.clone());
        (self.emit)(
            "channel-health",
            json!({ "channelId": id, "health": health }),
        );
    }

    fn has_token(&self, id: &str) -> bool {
        read_token((self.secrets)(id).as_ref())
            .ok()
            .flatten()
            .is_some()
    }

    /// Every channel with its health.
    ///
    /// # Errors
    /// Database errors.
    pub fn list(&self) -> Result<Vec<ChannelDto>, String> {
        let health = self
            .health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Ok(self
            .configs()?
            .into_iter()
            .map(|config| ChannelDto {
                has_token: self.has_token(&config.id),
                health: health.get(&config.id).cloned(),
                config,
            })
            .collect())
    }

    /// Start every enabled channel (app launch).
    pub async fn start_all(&self) {
        for config in self.configs().unwrap_or_default() {
            let id = config.id.clone();
            if let Err(error) = self.start(config).await {
                self.set_health(&id, &health_now(HealthState::Failed, &error));
            }
        }
    }

    /// Create or update a channel; a pasted token goes to the keychain.
    /// The channel restarts with the new settings.
    ///
    /// # Errors
    /// Invalid configuration or token, unknown workspace, keychain or
    /// database errors.
    pub async fn save(&self, input: ChannelInput) -> Result<ChannelDto, String> {
        let mut config = input.config;
        if config.id.trim().is_empty() {
            config.id = new_id();
        } else if !config.id.starts_with("ch_") || config.id.len() > 40 {
            return Err("invalid channel id".into());
        }
        let wildcard = self
            .state
            .allow_list(&config.id)?
            .iter()
            .any(|e| e.sender_id == next_agent_channels::policy::WILDCARD);
        config.validate(wildcard)?;
        self.app.workspace(&config.workspace_id)?;
        if let Some(token) = input
            .token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            if !telegram::api::valid_token(token) {
                return Err("that does not look like a Telegram bot token (expected `123456:ABC…` from @BotFather)".into());
            }
            (self.secrets)(&config.id)
                .set(token)
                .map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        self.app
            .store()?
            .channel_upsert(&ChannelRow {
                id: config.id.clone(),
                kind: config.kind.clone(),
                name: config.name.clone(),
                config: json,
            })
            .map_err(|e| e.to_string())?;
        if let Err(error) = self.start(config.clone()).await {
            self.set_health(&config.id, &health_now(HealthState::Failed, &error));
        }
        let health = self
            .health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&config.id)
            .cloned();
        Ok(ChannelDto {
            has_token: self.has_token(&config.id),
            health,
            config,
        })
    }

    /// Stop and delete a channel, its pairings, allow-list, activity and
    /// keychain token.
    ///
    /// # Errors
    /// Keychain or database errors.
    pub async fn delete(&self, id: &str) -> Result<bool, String> {
        self.stop(id).await;
        self.health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        (self.secrets)(id).delete().map_err(|e| e.to_string())?;
        self.app
            .store()?
            .channel_delete(id)
            .map_err(|e| e.to_string())
    }

    /// "Test connection": a pasted token, or the saved one for `id`.
    ///
    /// # Errors
    /// No token, a malformed or rejected token, or Telegram unreachable.
    pub async fn test(&self, id: Option<&str>, token: Option<&str>) -> Result<BotIdentity, String> {
        let token = if let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) {
            token.to_string()
        } else {
            let id = id.ok_or("paste a bot token first")?;
            read_token((self.secrets)(id).as_ref())?.ok_or("no bot token saved for this channel")?
        };
        telegram::test_token(&self.base_url, &token)
            .await
            .map_err(|e| e.to_string())
    }

    /// Restart one channel with its saved settings.
    ///
    /// # Errors
    /// Unknown channel, missing token or database errors.
    pub async fn restart(&self, id: &str) -> Result<(), String> {
        let config = self.config(id)?;
        self.start(config).await
    }

    async fn stop(&self, id: &str) {
        let running = self.running.lock().await.remove(id);
        if let Some(running) = running {
            running.supervised.stop().await;
            running.pump.abort();
            running.watcher.abort();
        }
        self.hub.unregister(id);
    }

    async fn start(&self, config: ChannelConfig) -> Result<(), String> {
        let id = config.id.clone();
        self.stop(&id).await;
        if !config.enabled {
            self.set_health(&id, &health_now(HealthState::Stopped, "disabled"));
            return Ok(());
        }
        let token = read_token((self.secrets)(&id).as_ref())?
            .ok_or("no bot token saved: paste one from @BotFather")?;
        let channel = Arc::new(
            TelegramChannel::new(config, &self.base_url, &token, telegram::POLL_TIMEOUT)
                .map_err(|e| e.to_string())?,
        );
        self.hub.register(channel.clone());
        let (tx, rx) = mpsc::channel(64);
        let supervised = supervise(channel, tx, Backoff::default());
        let pump = tokio::spawn(Arc::clone(&self.hub).pump(id.clone(), rx));
        let mut watch = supervised.health.clone();
        let health = Arc::clone(&self.health);
        let emit = Arc::clone(&self.emit);
        let watch_id = id.clone();
        let watcher = tokio::spawn(async move {
            loop {
                let current = watch.borrow_and_update().clone();
                health
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(watch_id.clone(), current.clone());
                emit(
                    "channel-health",
                    json!({ "channelId": watch_id, "health": current }),
                );
                if watch.changed().await.is_err() {
                    return;
                }
            }
        });
        self.running.lock().await.insert(
            id,
            Running {
                supervised,
                pump,
                watcher,
            },
        );
        Ok(())
    }

    /// Pending pairing requests of a channel.
    ///
    /// # Errors
    /// Database errors.
    pub fn pairings(&self, id: &str) -> Result<Vec<PairingRequest>, String> {
        self.hub.pending_pairings(id)
    }

    /// Approve or deny a pairing code. Desktop only: no chat command does
    /// this.
    ///
    /// # Errors
    /// Unknown or expired code, database errors.
    pub async fn resolve_pairing(
        &self,
        id: &str,
        code: &str,
        approve: bool,
    ) -> Result<PairingRequest, String> {
        self.hub.resolve_pairing(id, code, approve).await
    }

    /// The allow-list of a channel.
    ///
    /// # Errors
    /// Database errors.
    pub fn allow_list(&self, id: &str) -> Result<Vec<AllowEntry>, String> {
        self.state.allow_list(id)
    }

    /// Add a sender id (or `*`) to the allow-list.
    ///
    /// # Errors
    /// Empty or malformed id, database errors.
    pub fn allow_add(&self, id: &str, sender_id: &str, label: &str) -> Result<(), String> {
        let sender = sender_id.trim();
        let valid = sender == next_agent_channels::policy::WILDCARD
            || (!sender.is_empty()
                && sender.len() <= 64
                && sender
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_@.:".contains(c)));
        if !valid {
            return Err("enter a numeric Telegram user id (or * for anyone)".into());
        }
        self.config(id)?;
        self.state.allow_add(
            id,
            &AllowEntry {
                sender_id: sender.to_string(),
                label: label.trim().chars().take(64).collect(),
                source: "manual".into(),
                added_at: String::new(),
            },
        )
    }

    /// Remove a sender from the allow-list. Removing `*` while the policy is
    /// open is refused (open needs `*`).
    ///
    /// # Errors
    /// Database errors, or the open-policy rule.
    pub fn allow_remove(&self, id: &str, sender_id: &str) -> Result<bool, String> {
        if sender_id == next_agent_channels::policy::WILDCARD
            && self.config(id)?.dm_policy == next_agent_channels::DmPolicy::Open
        {
            return Err("switch the DM policy away from open before removing `*`".into());
        }
        self.state.allow_remove(id, sender_id)
    }

    /// Recent activity of a channel, newest first.
    ///
    /// # Errors
    /// Database errors.
    pub fn activity(
        &self,
        id: &str,
        limit: usize,
    ) -> Result<Vec<next_agent_channels::Activity>, String> {
        self.state.activity(id, limit.clamp(1, 500))
    }

    /// Scheduler hook ("deliver to channel"): send `text` to a paired chat
    /// or allowed group. Retrying with the same `idempotency_key` does not
    /// send twice. The memory-skills cron calls this for channel targets.
    ///
    /// # Errors
    /// Channel not running, chat not paired or allowed, or a send failure.
    pub async fn deliver(&self, delivery: &Delivery) -> Result<usize, String> {
        self.hub.deliver(delivery).await
    }
}

/// Runs channel jobs as desktop tasks and mirrors them to every window.
struct DesktopRunner {
    app: Arc<AppService>,
    emit: UiEmit,
}

/// What a stream message means for the chat.
fn observe(
    message: &StreamMessage,
    progress: &mpsc::UnboundedSender<TaskProgress>,
    outcome: &mut TaskOutcome,
    last_message: &mut String,
) {
    match message {
        StreamMessage::Event { event } => {
            let data = &event["data"];
            let text = |key: &str| data[key].as_str().unwrap_or_default().to_string();
            match event["kind"].as_str().unwrap_or_default() {
                "message_delta" => {
                    let _ = progress.send(TaskProgress::Delta(text("text")));
                }
                "message" => *last_message = text("text"),
                "plan_proposed" => *last_message = text("markdown"),
                "approval_requested" => {
                    let _ = progress.send(TaskProgress::WaitingForApproval {
                        approval_id: text("approval_id"),
                    });
                }
                "task_completed" => {
                    outcome.status = "completed".into();
                    let summary = text("summary");
                    outcome.answer = if last_message.trim().is_empty() {
                        summary
                    } else {
                        last_message.clone()
                    };
                }
                "task_failed" => {
                    outcome.status = "failed".into();
                    outcome.answer = text("error");
                }
                "task_cancelled" => outcome.status = "cancelled".into(),
                _ => {}
            }
        }
        StreamMessage::Notice {
            tone: "error",
            text,
            ..
        } if outcome.status.is_empty() => {
            outcome.answer.clone_from(text);
        }
        _ => {}
    }
}

impl TaskRunner for DesktopRunner {
    fn run(
        &self,
        job: ChannelJob,
        progress: mpsc::UnboundedSender<TaskProgress>,
        mut stop: StopToken,
    ) -> BoxFuture<'static, TaskOutcome> {
        let app = Arc::clone(&self.app);
        let emit = Arc::clone(&self.emit);
        Box::pin(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<StreamMessage>();
            let sink: Sink = Arc::new(move |message| {
                let _ = tx.send(message);
            });
            let request = TaskRequest {
                workspace_id: job.workspace_id.clone(),
                prompt: job.prompt.clone(),
                mode: job.mode.as_str().to_string(),
                profile: None,
            };
            let launch = ChannelLaunch {
                profile: channel_profile(&job.tools),
                approval_timeout: job.approval_timeout,
            };
            let task_id = match app.task_start_with(&request, sink, Some(&launch)) {
                Ok(id) => id,
                Err(error) => {
                    return TaskOutcome {
                        task_id: None,
                        status: "refused".into(),
                        answer: error,
                    }
                }
            };
            let _ = progress.send(TaskProgress::Started {
                task_id: task_id.clone(),
            });
            // Main window and overlay: show the task with its origin.
            let title: String = format!("{}: {}", job.origin, first_line(&job.prompt))
                .chars()
                .take(80)
                .collect();
            emit(
                "task-started",
                json!({ "taskId": task_id, "title": title, "mode": job.mode.as_str(), "origin": job.origin }),
            );
            let origin_notice = StreamMessage::Notice {
                task_id: task_id.clone(),
                tone: "info",
                text: format!(
                    "From {} (chat {}). Approvals happen here; no answer within {}s means deny.",
                    job.origin,
                    job.chat_id,
                    job.approval_timeout.as_secs()
                ),
            };
            emit(
                "task-stream",
                serde_json::to_value(&origin_notice).unwrap_or_default(),
            );

            let mut outcome = TaskOutcome {
                task_id: Some(task_id.clone()),
                ..TaskOutcome::default()
            };
            let mut last_message = String::new();
            let mut stopped = false;
            loop {
                tokio::select! {
                    message = rx.recv() => {
                        let Some(message) = message else { break };
                        emit("task-stream", serde_json::to_value(&message).unwrap_or_default());
                        observe(&message, &progress, &mut outcome, &mut last_message);
                    }
                    () = stop.stopped(), if !stopped => {
                        stopped = true;
                        app.task_cancel(&task_id);
                    }
                }
            }
            if outcome.status.is_empty() {
                outcome.status = "failed".into();
                if outcome.answer.is_empty() {
                    outcome.answer = "the task ended without a result".into();
                }
            }
            outcome
        })
    }
}

/// The new message's first line, for the task title.
fn first_line(prompt: &str) -> String {
    prompt
        .rsplit("\n---\n")
        .next()
        .and_then(|block| block.lines().next())
        .unwrap_or_default()
        .to_string()
}

// ---- Tauri commands -------------------------------------------------------

type Svc<'a> = tauri::State<'a, Arc<ChannelService>>;

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_list(service: Svc<'_>) -> Result<Vec<ChannelDto>, String> {
    service.list()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub async fn channel_save(service: Svc<'_>, input: ChannelInput) -> Result<ChannelDto, String> {
    service.save(input).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub async fn channel_delete(service: Svc<'_>, id: String) -> Result<bool, String> {
    service.delete(&id).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub async fn channel_test(
    service: Svc<'_>,
    id: Option<String>,
    token: Option<String>,
) -> Result<BotIdentity, String> {
    service.test(id.as_deref(), token.as_deref()).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub async fn channel_restart(service: Svc<'_>, id: String) -> Result<(), String> {
    service.restart(&id).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_pairings(service: Svc<'_>, id: String) -> Result<Vec<PairingRequest>, String> {
    service.pairings(&id)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub async fn channel_pairing_resolve(
    service: Svc<'_>,
    id: String,
    code: String,
    approve: bool,
) -> Result<PairingRequest, String> {
    service.resolve_pairing(&id, &code, approve).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_allow_list(service: Svc<'_>, id: String) -> Result<Vec<AllowEntry>, String> {
    service.allow_list(&id)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_allow_add(
    service: Svc<'_>,
    id: String,
    sender_id: String,
    label: String,
) -> Result<(), String> {
    service.allow_add(&id, &sender_id, &label)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_allow_remove(
    service: Svc<'_>,
    id: String,
    sender_id: String,
) -> Result<bool, String> {
    service.allow_remove(&id, &sender_id)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.
pub fn channel_activity(
    service: Svc<'_>,
    id: String,
    limit: Option<usize>,
) -> Result<Vec<next_agent_channels::Activity>, String> {
    service.activity(&id, limit.unwrap_or(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_agent_core::router::MemorySecret;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN: &str = "123456:ABCdefGHIjklMNOpqrSTUvwxYZ_0123";

    struct Fx {
        service: Arc<ChannelService>,
        events: Arc<Mutex<Vec<(String, Value)>>>,
        secrets: Arc<Mutex<HashMap<String, Arc<MemorySecret>>>>,
        workspace_id: String,
        _dir: tempfile::TempDir,
    }

    fn fixture(base: &str) -> Fx {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("ws")).expect("ws");
        let app = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("app"));
        let workspace_id = app
            .workspace_open(dir.path().join("ws").to_str().expect("utf8"))
            .expect("open")
            .id;
        let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = Arc::clone(&events);
        let secrets: Arc<Mutex<HashMap<String, Arc<MemorySecret>>>> = Arc::default();
        let store = Arc::clone(&secrets);
        let service = ChannelService::with(
            app,
            Arc::new(move |name, value| sink.lock().expect("lock").push((name.to_string(), value))),
            Arc::new(move |id: &str| {
                Arc::clone(
                    store
                        .lock()
                        .expect("lock")
                        .entry(id.to_string())
                        .or_default(),
                ) as Arc<dyn SecretStore>
            }),
            base,
        )
        .expect("service");
        Fx {
            service,
            events,
            secrets,
            workspace_id,
            _dir: dir,
        }
    }

    fn input(fx: &Fx, token: Option<&str>) -> ChannelInput {
        ChannelInput {
            config: ChannelConfig {
                workspace_id: fx.workspace_id.clone(),
                enabled: false,
                ..ChannelConfig::default()
            },
            token: token.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn token_goes_to_the_keychain_not_sqlite() {
        let fx = fixture(DEFAULT_BASE_URL);
        let saved = fx
            .service
            .save(input(&fx, Some(TOKEN)))
            .await
            .expect("save");
        assert!(saved.has_token);
        let id = saved.config.id.clone();
        assert_eq!(
            fx.secrets.lock().expect("lock")[&id]
                .get()
                .expect("get")
                .as_deref(),
            Some(TOKEN)
        );
        let rows = fx
            .service
            .app
            .store()
            .expect("store")
            .channel_rows()
            .expect("rows");
        assert!(
            rows.iter().all(|r| !r.config.contains("ABCdef")),
            "token must not be in SQLite"
        );
        let listed = serde_json::to_string(&fx.service.list().expect("list")).expect("json");
        assert!(!listed.contains("ABCdef"), "token must not reach the UI");
        // Saving again without a token keeps the stored one.
        let again = fx
            .service
            .save(ChannelInput {
                config: saved.config.clone(),
                token: None,
            })
            .await
            .expect("resave");
        assert!(again.has_token);
        fx.service.delete(&id).await.expect("delete");
        assert_eq!(
            fx.secrets.lock().expect("lock")[&id].get().expect("get"),
            None
        );
    }

    #[test]
    fn debug_output_redacts_the_token() {
        let input = ChannelInput {
            config: ChannelConfig::default(),
            token: Some(TOKEN.into()),
        };
        let text = format!("{input:?}");
        assert!(!text.contains("ABCdef"), "{text}");
        assert!(text.contains("<redacted>"));
    }

    #[tokio::test]
    async fn save_validates() {
        let fx = fixture(DEFAULT_BASE_URL);
        assert!(fx.service.save(input(&fx, Some("nope"))).await.is_err());
        let mut bad = input(&fx, None);
        bad.config.workspace_id = "ws_missing".into();
        assert!(fx.service.save(bad).await.is_err());
        let mut open = input(&fx, None);
        open.config.dm_policy = next_agent_channels::DmPolicy::Open;
        assert!(fx.service.save(open).await.unwrap_err().contains('*'));
        let json =
            json!({ "config": { "workspaceId": fx.workspace_id, "mode": "bypassPermissions" } });
        assert!(
            serde_json::from_value::<ChannelInput>(json).is_err(),
            "bypass cannot even be deserialized"
        );
    }

    #[tokio::test]
    async fn channel_tasks_refuse_auto_and_bypass_modes() {
        let fx = fixture(DEFAULT_BASE_URL);
        for mode in ["auto", "bypassPermissions"] {
            let request = TaskRequest {
                workspace_id: fx.workspace_id.clone(),
                prompt: "x".into(),
                mode: mode.into(),
                profile: None,
            };
            let launch = ChannelLaunch {
                profile: channel_profile(&[]),
                approval_timeout: Duration::from_secs(1),
            };
            let error = fx
                .service
                .app
                .task_start_with(&request, Arc::new(|_| {}), Some(&launch))
                .expect_err(mode);
            assert!(error.contains("never"), "{error}");
        }
    }

    #[tokio::test]
    async fn runner_mirrors_the_task_to_the_ui_with_its_origin() {
        let fx = fixture(DEFAULT_BASE_URL);
        let runner = DesktopRunner {
            app: Arc::clone(&fx.service.app),
            emit: Arc::clone(&fx.service.emit),
        };
        let (progress, mut updates) = mpsc::unbounded_channel();
        let job = ChannelJob {
            channel_id: "ch_1".into(),
            workspace_id: fx.workspace_id.clone(),
            mode: next_agent_channels::ChannelMode::Plan,
            tools: vec!["read_file".into(), "list_dir".into()],
            approval_timeout: Duration::from_millis(200),
            session_key: "s".into(),
            prompt: "x\n---\nlist the files\n<<<END".into(),
            origin: "Telegram · @alice".into(),
            chat_id: "42".into(),
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            runner.run(job, progress, StopToken::never()),
        )
        .await
        .expect("finishes");
        assert_eq!(outcome.status, "completed", "{outcome:?}");
        assert!(matches!(
            updates.recv().await,
            Some(TaskProgress::Started { .. })
        ));
        let events = fx.events.lock().expect("lock").clone();
        let started = events
            .iter()
            .find(|(name, _)| name == "task-started")
            .expect("task-started");
        assert_eq!(started.1["origin"], "Telegram · @alice");
        assert!(started.1["title"]
            .as_str()
            .is_some_and(|t| t.contains("list the files")));
        assert!(events.iter().any(|(name, v)| name == "task-stream"
            && v["type"] == "notice"
            && v["text"]
                .as_str()
                .is_some_and(|t| t.contains("Telegram · @alice"))));
        // The scripted demo agent writes a file: denied in plan mode and by
        // the profile (write_file is not in this job's tools).
        let denied = events
            .iter()
            .filter(|(name, v)| {
                name == "task-stream"
                    && v["event"]["kind"] == "permission_decided"
                    && v["event"]["data"]["decision"] == "deny"
            })
            .count();
        assert!(denied >= 1);
    }

    #[tokio::test]
    async fn starting_without_a_token_reports_failed_and_test_uses_the_api() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/bot{TOKEN}/getMe")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": { "id": 1, "username": "NextBot", "first_name": "N" } })))
            .mount(&server)
            .await;
        let fx = fixture(&server.uri());
        let mut enabled = input(&fx, None);
        enabled.config.enabled = true;
        let saved = fx.service.save(enabled).await.expect("save");
        assert_eq!(saved.health.map(|h| h.state), Some(HealthState::Failed));
        assert_eq!(
            fx.service
                .test(None, Some(TOKEN))
                .await
                .expect("test")
                .username,
            "NextBot"
        );
        assert!(
            fx.service.test(Some(&saved.config.id), None).await.is_err(),
            "no saved token"
        );
    }

    #[tokio::test]
    async fn allow_list_editing_rules() {
        let fx = fixture(DEFAULT_BASE_URL);
        let saved = fx.service.save(input(&fx, None)).await.expect("save");
        let id = saved.config.id.clone();
        assert!(fx.service.allow_add(&id, "../etc", "").is_err());
        fx.service.allow_add(&id, "42", "@alice").expect("add");
        fx.service
            .allow_add(&id, "*", "everyone")
            .expect("wildcard");
        let mut open = saved.config.clone();
        open.id.clone_from(&id);
        open.dm_policy = next_agent_channels::DmPolicy::Open;
        fx.service
            .save(ChannelInput {
                config: open,
                token: None,
            })
            .await
            .expect("open with *");
        assert!(fx.service.allow_remove(&id, "*").is_err(), "open needs *");
        assert!(fx.service.allow_remove(&id, "42").expect("remove"));
    }
}
