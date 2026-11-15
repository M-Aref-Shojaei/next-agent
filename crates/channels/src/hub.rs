//! The channel hub: every inbound message passes through here.
//!
//! inbound → idempotency → access (DM policy and pairing, or group gating)
//! → commands (`/stop`, `/new`) → session routing → per-session queue →
//! a task in the normal pipeline ([`TaskRunner`]: `spawn_task`, broker,
//! permission engine) → the task's final answer back to the chat.
//!
//! The hub never approves anything. A task waiting for approval only makes
//! the chat say so; the answer comes from the desktop or the overlay, and
//! no answer within the timeout is a deny (enforced by the runner).

use crate::channel::{BoxFuture, Channel, ChannelError};
use crate::config::{ChannelMode, DmPolicy, GroupPolicy};
use crate::outbound::{self, Preview};
use crate::pairing::{self, PairingOutcome, PairingRequest};
use crate::policy::{self, DmAccess, GroupAccess, GroupFacts};
use crate::queue::{SessionQueues, StopToken};
use crate::routing::{self, RouteInput};
use crate::state::{Activity, AllowEntry, ChannelState};
use crate::types::{ChatKind, Inbound};
use crate::untrusted::{self, Turn};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Turns of conversation kept per session and replayed (wrapped as
/// untrusted) into the next task's prompt. In memory only.
pub const HISTORY_TURNS: usize = 6;
const HISTORY_CHARS: usize = 1_500;
const LOG_CHARS: usize = 300;
const TYPING_EVERY: Duration = Duration::from_secs(4);

/// The chat line shown while a task waits for a desktop approval.
pub const WAITING_FOR_APPROVAL: &str = "⏳ Waiting for approval on the desktop…";

/// One task to run for a channel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelJob {
    pub channel_id: String,
    pub channel_kind: String,
    pub workspace_id: String,
    pub mode: ChannelMode,
    pub tools: Vec<String>,
    pub approval_timeout: Duration,
    pub session_key: String,
    /// Full task prompt, with the message (and the session's recent turns)
    /// wrapped as untrusted.
    pub prompt: String,
    /// Timeline label, e.g. `Telegram · @alice`.
    pub origin: String,
    pub chat_id: String,
    pub sender_id: String,
}

/// Progress a runner reports while the task runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskProgress {
    Started {
        task_id: String,
    },
    /// Streamed answer text.
    Delta(String),
    /// An approval card is up on the desktop.
    WaitingForApproval {
        approval_id: String,
    },
}

/// How a task ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskOutcome {
    pub task_id: Option<String>,
    /// `completed`, `failed`, `cancelled` or `refused`.
    pub status: String,
    /// The final answer (or the error for `failed` / `refused`).
    pub answer: String,
}

/// Runs a [`ChannelJob`] as a task. The desktop implements it over
/// `AppService::task_start`; tests use a fake.
pub trait TaskRunner: Send + Sync {
    fn run(
        &self,
        job: ChannelJob,
        progress: mpsc::UnboundedSender<TaskProgress>,
        stop: StopToken,
    ) -> BoxFuture<'static, TaskOutcome>;
}

/// What happened to one inbound message (for tests and logs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    Duplicate,
    UnknownChannel,
    Dropped(&'static str),
    Pairing(PairingOutcome),
    Command(&'static str),
    Unsupported,
    Queued(String),
}

/// Notifications for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubEvent {
    PairingRequested { channel_id: String },
    Activity { channel_id: String },
}

/// A message the scheduler (or anything else in the app) wants sent to a
/// chat: the "deliver to channel" hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub channel_id: String,
    pub chat_id: String,
    pub text: String,
    /// Same key, same message: a retried job is delivered once.
    pub idempotency_key: Option<String>,
}

type Notify = Arc<dyn Fn(HubEvent) + Send + Sync>;
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

struct Work {
    channel: Arc<dyn Channel>,
    message: Inbound,
    job: ChannelJob,
}

/// See the module docs.
pub struct Hub {
    state: Arc<dyn ChannelState>,
    runner: Arc<dyn TaskRunner>,
    channels: Mutex<HashMap<String, Arc<dyn Channel>>>,
    queues: SessionQueues<Work>,
    history: Mutex<HashMap<String, VecDeque<Turn>>>,
    notify: Mutex<Option<Notify>>,
    clock: Clock,
    throttle: Duration,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let mut cut: String = text.chars().take(limit).collect();
        cut.push('…');
        cut
    }
}

/// A chat command, after access checks. `/cmd@bot` counts as `/cmd`.
fn command(text: &str) -> Option<&'static str> {
    let first = text.split_whitespace().next()?;
    let name = first
        .strip_prefix('/')?
        .split('@')
        .next()?
        .to_ascii_lowercase();
    Some(match name.as_str() {
        "stop" => "stop",
        "new" | "reset" => "new",
        "start" | "help" => "help",
        _ => return None,
    })
}

const HELP: &str = "Next Agent is listening. Send a request in plain text; I reply when the task finishes. Changes and commands need approval on the desktop. /stop cancels the current task, /new starts a fresh conversation.";

impl Hub {
    /// Build a hub. Call [`Hub::register`] for each channel, then feed
    /// inbound messages with [`Hub::pump`] or [`Hub::handle`].
    #[must_use]
    pub fn new(state: Arc<dyn ChannelState>, runner: Arc<dyn TaskRunner>) -> Arc<Self> {
        Self::build(
            state,
            runner,
            Arc::new(now_secs),
            outbound::PREVIEW_THROTTLE,
        )
    }

    /// [`Hub::new`] with an injected clock and preview throttle (tests).
    #[must_use]
    pub fn build(
        state: Arc<dyn ChannelState>,
        runner: Arc<dyn TaskRunner>,
        clock: Clock,
        throttle: Duration,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            Self {
                state,
                runner,
                channels: Mutex::default(),
                queues: SessionQueues::new(move |work: Work, stop: StopToken| {
                    let weak = weak.clone();
                    Box::pin(async move {
                        if let Some(hub) = weak.upgrade() {
                            hub.run(work, stop).await;
                        }
                    })
                }),
                history: Mutex::default(),
                notify: Mutex::default(),
                clock,
                throttle,
            }
        })
    }

    /// Receive UI notifications.
    pub fn set_notify(&self, notify: impl Fn(HubEvent) + Send + Sync + 'static) {
        *self.notify.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(notify));
    }

    fn emit(&self, event: HubEvent) {
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(notify) = notify {
            notify(event);
        }
    }

    /// Add or replace a channel (by its config id).
    pub fn register(&self, channel: Arc<dyn Channel>) {
        let id = channel.config().id.clone();
        self.lock_channels().insert(id, channel);
    }

    pub fn unregister(&self, channel_id: &str) -> bool {
        self.lock_channels().remove(channel_id).is_some()
    }

    #[must_use]
    pub fn channel(&self, channel_id: &str) -> Option<Arc<dyn Channel>> {
        self.lock_channels().get(channel_id).cloned()
    }

    fn lock_channels(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<dyn Channel>>> {
        self.channels.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn log(
        &self,
        channel_id: &str,
        direction: &str,
        kind: &str,
        peer: &str,
        text: &str,
        task_id: Option<String>,
    ) {
        let _ = self.state.log(&Activity {
            ts: String::new(),
            channel_id: channel_id.to_string(),
            direction: direction.into(),
            kind: kind.into(),
            peer: peer.to_string(),
            text: clip(text, LOG_CHARS),
            task_id,
        });
        self.emit(HubEvent::Activity {
            channel_id: channel_id.to_string(),
        });
    }

    /// Handle inbound messages from one channel until `inbound` closes.
    pub async fn pump(self: Arc<Self>, channel_id: String, mut inbound: mpsc::Receiver<Inbound>) {
        while let Some(message) = inbound.recv().await {
            self.handle(&channel_id, message).await;
        }
    }

    fn allow_ids(&self, channel_id: &str) -> Vec<String> {
        self.state
            .allow_list(channel_id)
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.sender_id)
            .collect()
    }

    /// Decide and act on one inbound message.
    pub async fn handle(&self, channel_id: &str, message: Inbound) -> Handled {
        let Some(channel) = self.channel(channel_id) else {
            return Handled::UnknownChannel;
        };
        if !self
            .state
            .claim(
                &outbound::inbound_key(&channel.key_scope(), &message.event_id),
                "",
            )
            .unwrap_or(true)
        {
            return Handled::Duplicate;
        }
        let config = channel.config().clone();
        let peer = format!("{} ({})", message.sender_label, message.sender_id);
        if let Err(handled) = self.check_access(channel.as_ref(), &message, &peer).await {
            return handled;
        }

        let route = RouteInput {
            channel: &config.kind,
            account: &config.id,
            chat_kind: message.kind(),
            peer_id: match message.kind() {
                ChatKind::Direct => &message.sender_id,
                ChatKind::Group => &message.chat_id,
            },
            thread_id: message.thread_id.as_deref(),
        };
        let session = routing::session_key(&route, config.dm_scope, &config.identity_links);

        if let Some(name) = command(&message.text) {
            self.run_command(channel.as_ref(), &message, name, &session)
                .await;
            self.log(channel_id, "in", "command", &peer, &message.text, None);
            return Handled::Command(name);
        }

        if message.text.trim().is_empty() {
            if message.media.is_some() {
                let _ = outbound::send_reply(
                    channel.as_ref(),
                    self.state.as_ref(),
                    &message,
                    "Media is not supported yet. Please send your request as text.",
                )
                .await;
                return Handled::Unsupported;
            }
            return Handled::Dropped("empty message");
        }

        self.log(channel_id, "in", "message", &peer, &message.text, None);
        let mut origin = format!("{} · {}", config.name, channel.pairing_label(&message));
        if message.kind() == ChatKind::Group {
            let _ = write!(origin, " in group {}", message.chat_id);
        }
        let job = ChannelJob {
            channel_id: channel_id.to_string(),
            channel_kind: config.kind.clone(),
            workspace_id: config.workspace_id.clone(),
            mode: config.mode,
            tools: config.tools.clone(),
            approval_timeout: Duration::from_secs(config.approval_timeout_secs),
            session_key: session.clone(),
            // Built when the job runs, so it sees the answers to earlier
            // messages of the same session.
            prompt: String::new(),
            origin,
            chat_id: message.chat_id.clone(),
            sender_id: message.sender_id.clone(),
        };
        self.queues.push(
            &session,
            Work {
                channel,
                message,
                job,
            },
        );
        Handled::Queued(session)
    }

    /// DM policy and pairing, or group gating. `Err` carries the outcome
    /// when the message stops here.
    async fn check_access(
        &self,
        channel: &dyn Channel,
        message: &Inbound,
        peer: &str,
    ) -> Result<(), Handled> {
        let config = channel.config();
        let channel_id = config.id.as_str();
        let allow = self.allow_ids(channel_id);
        let verdict = match message.kind() {
            ChatKind::Direct => {
                match policy::dm_access(config.dm_policy, &allow, &message.sender_id) {
                    DmAccess::Allow => Ok(()),
                    DmAccess::Deny(reason) => Err(reason),
                    DmAccess::NeedsPairing => {
                        let outcome = self.request_pairing(channel, message);
                        // Answer only when a code is issued: repeats and a
                        // full queue stay silent, so a flood of unpaired
                        // senders cannot spend the bot's rate limit.
                        if matches!(outcome, PairingOutcome::Issued(_)) {
                            let reply = pairing::pairing_reply(&outcome, &message.sender_id);
                            let _ =
                                outbound::send_reply(channel, self.state.as_ref(), message, &reply)
                                    .await;
                            self.log(channel_id, "in", "pairing", peer, &message.text, None);
                            self.emit(HubEvent::PairingRequested {
                                channel_id: channel_id.to_string(),
                            });
                        } else if outcome == PairingOutcome::Full {
                            // Silent to the sender, visible to the owner.
                            self.log(channel_id, "in", "denied", peer, "pairing queue full", None);
                        }
                        return Err(Handled::Pairing(outcome));
                    }
                }
            }
            ChatKind::Group => {
                let facts = GroupFacts {
                    group_id: message.chat_id.clone(),
                    sender_id: message.sender_id.clone(),
                    mentioned: message.mentioned,
                    reply_to_bot: message.reply_to_bot,
                };
                match policy::group_access(config, &allow, &facts) {
                    GroupAccess::Allow => Ok(()),
                    GroupAccess::Skip(reason) => Err(reason),
                }
            }
        };
        verdict.map_err(|reason| {
            // Unmentioned group chatter is not worth a log line.
            if reason != "not mentioned" {
                self.log(channel_id, "in", "denied", peer, reason, None);
            }
            Handled::Dropped(reason)
        })
    }

    /// `/stop`, `/new`, `/help`.
    async fn run_command(
        &self,
        channel: &dyn Channel,
        message: &Inbound,
        name: &str,
        session: &str,
    ) {
        let reply = match name {
            "stop" if self.queues.stop(session) => "Stopped.",
            "stop" => "Nothing is running.",
            "new" => {
                self.queues.stop(session);
                self.lock_history().remove(session);
                "Started a new conversation."
            }
            _ => HELP,
        };
        let _ = outbound::send_reply(channel, self.state.as_ref(), message, reply).await;
    }

    fn lock_history(&self) -> std::sync::MutexGuard<'_, HashMap<String, VecDeque<Turn>>> {
        self.history.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn request_pairing(&self, channel: &dyn Channel, message: &Inbound) -> PairingOutcome {
        let channel_id = &channel.config().id;
        let mut pending = self.state.pairings(channel_id).unwrap_or_default();
        let template = PairingRequest {
            channel_id: channel_id.clone(),
            sender_id: message.sender_id.clone(),
            chat_id: message.chat_id.clone(),
            label: clip(&untrusted::sanitize(&channel.pairing_label(message)), 64),
            code: String::new(),
            created_at: 0,
            last_seen_at: 0,
        };
        let outcome = pairing::request(
            &mut pending,
            template,
            (self.clock)(),
            &mut pairing::random_code,
        );
        let _ = self.state.set_pairings(channel_id, &pending);
        outcome
    }

    /// Pending, unexpired pairing requests of a channel.
    ///
    /// # Errors
    /// Storage failures.
    pub fn pending_pairings(&self, channel_id: &str) -> Result<Vec<PairingRequest>, String> {
        let mut pending = self.state.pairings(channel_id)?;
        let before = pending.len();
        pairing::prune(&mut pending, (self.clock)());
        if pending.len() != before {
            self.state.set_pairings(channel_id, &pending)?;
        }
        Ok(pending)
    }

    /// Approve or deny a pairing code (desktop only). Approving adds the
    /// sender to the allow-list and tells them in the chat.
    ///
    /// # Errors
    /// Unknown or expired code, or storage failures.
    pub async fn resolve_pairing(
        &self,
        channel_id: &str,
        code: &str,
        approve: bool,
    ) -> Result<PairingRequest, String> {
        let mut pending = self.state.pairings(channel_id)?;
        let request = pairing::take(&mut pending, code, (self.clock)()).ok_or_else(|| {
            "no pending pairing request with that code (it may have expired)".to_string()
        })?;
        self.state.set_pairings(channel_id, &pending)?;
        if approve {
            self.state.allow_add(
                channel_id,
                &AllowEntry {
                    sender_id: request.sender_id.clone(),
                    label: request.label.clone(),
                    source: "pairing".into(),
                    added_at: String::new(),
                },
            )?;
            if let Some(channel) = self.channel(channel_id) {
                let _ = channel
                    .send(crate::types::Outbound {
                        chat_id: request.chat_id.clone(),
                        text: "Next Agent: you are paired. Send a request any time.".into(),
                        ..crate::types::Outbound::default()
                    })
                    .await;
            }
        }
        let verdict = if approve {
            "pairing approved"
        } else {
            "pairing denied"
        };
        self.log(
            channel_id,
            "system",
            "pairing",
            &format!("{} ({})", request.label, request.sender_id),
            verdict,
            None,
        );
        Ok(request)
    }

    /// Send a message to a chat outside a conversation: the "deliver to
    /// channel" hook for the scheduler. Only chats that may talk to the bot
    /// (an allow-listed DM sender, or an allowed group) can receive.
    ///
    /// # Errors
    /// Unknown channel, a chat that is not allowed, or a send failure.
    pub async fn deliver(&self, delivery: &Delivery) -> Result<usize, String> {
        let channel = self
            .channel(&delivery.channel_id)
            .ok_or_else(|| format!("channel {} is not running", delivery.channel_id))?;
        let config = channel.config();
        let chat = &delivery.chat_id;
        // A paired DM (Telegram: the chat id is the user id) while DMs are
        // on, or a listed group while groups are on. Policies are honoured.
        let dm = config.dm_policy != DmPolicy::Disabled
            && self
                .allow_ids(&delivery.channel_id)
                .iter()
                .any(|id| id == chat);
        let group =
            config.group_policy != GroupPolicy::Disabled && config.groups.iter().any(|g| g == chat);
        if !(dm || group) {
            return Err(format!(
                "chat {} is not paired or allowed on this channel",
                delivery.chat_id
            ));
        }
        let limit = channel.capabilities().text_chunk_limit;
        let mut sent = 0;
        for (part, chunk) in crate::chunk::chunk_text(&delivery.text, limit)
            .into_iter()
            .enumerate()
        {
            let key = delivery
                .idempotency_key
                .as_ref()
                .map(|k| format!("deliver:{}:{k}:{part}", delivery.channel_id));
            // Claim before sending, so two concurrent retries cannot both
            // send; a failed send releases the claim for the next retry.
            if let Some(key) = &key {
                if !self.state.claim(key, "pending")? {
                    continue;
                }
            }
            let result = channel
                .send(crate::types::Outbound {
                    chat_id: delivery.chat_id.clone(),
                    text: chunk,
                    ..crate::types::Outbound::default()
                })
                .await;
            if let Err(error) = result {
                if let Some(key) = &key {
                    self.state.release(key)?;
                }
                return Err(error.to_string());
            }
            sent += 1;
        }
        self.log(
            &delivery.channel_id,
            "out",
            "delivery",
            &delivery.chat_id,
            &delivery.text,
            None,
        );
        Ok(sent)
    }

    async fn run(&self, work: Work, stop: StopToken) {
        let Work {
            channel,
            message,
            mut job,
        } = work;
        let history: Vec<Turn> = self
            .lock_history()
            .get(&job.session_key)
            .map_or_else(Vec::new, |h| h.iter().cloned().collect());
        job.prompt = untrusted::task_prompt(&channel.config().name, &message, &history);
        let channel_id = job.channel_id.clone();
        let session = job.session_key.clone();
        let peer = format!("{} ({})", message.sender_label, message.sender_id);
        let typing = channel.capabilities().typing.then(|| {
            let channel = Arc::clone(&channel);
            let chat = message.chat_id.clone();
            tokio::spawn(async move {
                loop {
                    let _ = channel.typing(chat.clone()).await;
                    tokio::time::sleep(TYPING_EVERY).await;
                }
            })
        });

        let (progress_tx, mut progress) = mpsc::unbounded_channel();
        let task = self.runner.run(job, progress_tx, stop);
        tokio::pin!(task);
        let mut preview = Preview::new(channel.as_ref(), self.state.as_ref(), &message)
            .with_throttle(self.throttle);
        let mut announced = HashSet::new();
        let mut task_id = None;
        let outcome = loop {
            tokio::select! {
                outcome = &mut task => break outcome,
                Some(update) = progress.recv() => match update {
                    TaskProgress::Started { task_id: id } => task_id = Some(id),
                    TaskProgress::Delta(text) => preview.push(&text).await,
                    TaskProgress::WaitingForApproval { approval_id } => {
                        if announced.insert(approval_id) {
                            let _ = channel.send(channel.reply_target(&message, WAITING_FOR_APPROVAL.into())).await;
                        }
                    }
                },
            }
        };
        if let Some(typing) = typing {
            typing.abort();
        }
        let task_id = outcome.task_id.clone().or(task_id);
        let text = match outcome.status.as_str() {
            "completed" if outcome.answer.trim().is_empty() => "Done.".to_string(),
            "completed" => outcome.answer.clone(),
            "cancelled" => "Stopped.".to_string(),
            "refused" => {
                "This request could not be started. The owner can see why in the desktop app."
                    .to_string()
            }
            _ => format!("The task failed: {}", clip(&outcome.answer, 500)),
        };
        if outcome.status == "refused" {
            self.log(&channel_id, "system", "error", &peer, &outcome.answer, None);
        }
        match preview.finish(&text).await {
            Ok(_) => self.log(&channel_id, "out", "message", &peer, &text, task_id.clone()),
            Err(error) => self.log(
                &channel_id,
                "out",
                "error",
                &peer,
                &send_error(&error),
                task_id.clone(),
            ),
        }
        if outcome.status == "completed" {
            let mut history = self.lock_history();
            let turns = history.entry(session).or_default();
            turns.push_back(Turn {
                sender: untrusted::sender_of(&message),
                asked: clip(&message.text, HISTORY_CHARS),
                answered: clip(&outcome.answer, HISTORY_CHARS),
            });
            while turns.len() > HISTORY_TURNS {
                turns.pop_front();
            }
        }
    }
}

fn send_error(error: &ChannelError) -> String {
    format!("could not send the reply: {error}")
}
