//! Hub end to end with a fake channel and a fake task runner: access,
//! pairing, commands, routing, ordering, idempotency, streaming previews,
//! approval notices and the scheduler delivery hook.

use next_agent_channels::channel::{BoxFuture, ConnectedHook};
use next_agent_channels::{
    BotIdentity, Capabilities, Channel, ChannelConfig, ChannelError, ChannelJob, ChannelState,
    ChatKind, Delivery, DmPolicy, GroupPolicy, Handled, Hub, Inbound, MemoryState, MessageRef,
    Outbound, StopToken, TaskOutcome, TaskProgress, TaskRunner,
};
use next_agent_channels::pairing::PairingOutcome;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sent {
    Send { chat: String, text: String, reply_to: Option<String> },
    Edit { id: String, text: String },
}

struct FakeChannel {
    config: ChannelConfig,
    sent: Mutex<Vec<Sent>>,
    next_id: AtomicU64,
    limit: usize,
}

impl FakeChannel {
    fn new(config: ChannelConfig) -> Arc<Self> {
        Arc::new(Self { config, sent: Mutex::default(), next_id: AtomicU64::new(100), limit: 4096 })
    }
    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().expect("lock").clone()
    }
    fn texts(&self) -> Vec<String> {
        self.sent()
            .into_iter()
            .map(|s| match s {
                Sent::Send { text, .. } | Sent::Edit { text, .. } => text,
            })
            .collect()
    }
}

impl Channel for FakeChannel {
    fn config(&self) -> &ChannelConfig {
        &self.config
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { text_chunk_limit: self.limit, edit: true, typing: false, threads: true, media: false }
    }
    fn probe(&self) -> BoxFuture<'_, Result<BotIdentity, ChannelError>> {
        Box::pin(async { Ok(BotIdentity::default()) })
    }
    fn start(&self, _i: mpsc::Sender<Inbound>, _c: ConnectedHook) -> BoxFuture<'_, Result<(), ChannelError>> {
        Box::pin(std::future::pending())
    }
    fn send(&self, m: Outbound) -> BoxFuture<'_, Result<MessageRef, ChannelError>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        self.sent.lock().expect("lock").push(Sent::Send { chat: m.chat_id.clone(), text: m.text, reply_to: m.reply_to });
        Box::pin(async move { Ok(MessageRef { chat_id: m.chat_id, message_id: id }) })
    }
    fn edit(&self, t: MessageRef, text: String) -> BoxFuture<'_, Result<(), ChannelError>> {
        self.sent.lock().expect("lock").push(Sent::Edit { id: t.message_id, text });
        Box::pin(async { Ok(()) })
    }
    fn typing(&self, _c: String) -> BoxFuture<'_, Result<(), ChannelError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Records jobs; answers `echo: <message>` after the scripted progress.
#[derive(Default)]
struct FakeRunner {
    jobs: Mutex<Vec<ChannelJob>>,
    progress: Mutex<Vec<TaskProgress>>,
    delay: Mutex<Duration>,
    done: Mutex<Option<mpsc::UnboundedSender<String>>>,
}

impl TaskRunner for FakeRunner {
    fn run(&self, job: ChannelJob, progress: mpsc::UnboundedSender<TaskProgress>, mut stop: StopToken) -> BoxFuture<'static, TaskOutcome> {
        self.jobs.lock().expect("lock").push(job.clone());
        let script = self.progress.lock().expect("lock").clone();
        let delay = *self.delay.lock().expect("lock");
        let done = self.done.lock().expect("lock").clone();
        Box::pin(async move {
            let _ = progress.send(TaskProgress::Started { task_id: "tsk_1".into() });
            for p in script {
                let _ = progress.send(p);
                tokio::task::yield_now().await;
            }
            let last = job.prompt.rsplit("---\n").next().unwrap_or_default().lines().next().unwrap_or_default().to_string();
            let outcome = tokio::select! {
                () = tokio::time::sleep(delay) => TaskOutcome { task_id: Some("tsk_1".into()), status: "completed".into(), answer: format!("echo: {last}") },
                () = stop.stopped() => TaskOutcome { task_id: Some("tsk_1".into()), status: "cancelled".into(), answer: String::new() },
            };
            if let Some(done) = done {
                let _ = done.send(last);
            }
            outcome
        })
    }
}

struct Rig {
    hub: Arc<Hub>,
    channel: Arc<FakeChannel>,
    runner: Arc<FakeRunner>,
    state: Arc<MemoryState>,
    done: mpsc::UnboundedReceiver<String>,
    clock: Arc<AtomicU64>,
}

fn rig(config: ChannelConfig) -> Rig {
    let state = Arc::new(MemoryState::default());
    let runner = Arc::new(FakeRunner::default());
    let (tx, done) = mpsc::unbounded_channel();
    *runner.done.lock().expect("lock") = Some(tx);
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = Arc::clone(&clock);
    let hub = Hub::build(
        state.clone() as Arc<dyn ChannelState>,
        runner.clone() as Arc<dyn TaskRunner>,
        Arc::new(move || c.load(Ordering::SeqCst)),
        Duration::ZERO,
    );
    let channel = FakeChannel::new(config);
    hub.register(channel.clone());
    Rig { hub, channel, runner, state, done, clock }
}

fn config() -> ChannelConfig {
    ChannelConfig { id: "ch_1".into(), workspace_id: "ws_1".into(), ..ChannelConfig::default() }
}

fn dm(event: u32, sender: &str, text: &str) -> Inbound {
    Inbound {
        event_id: event.to_string(),
        chat_id: sender.into(),
        chat_kind: Some(ChatKind::Direct),
        sender_id: sender.into(),
        sender_label: format!("@{sender}"),
        message_id: format!("m{event}"),
        text: text.into(),
        ..Inbound::default()
    }
}

fn group(event: u32, sender: &str, text: &str, mentioned: bool, reply: bool) -> Inbound {
    Inbound { chat_id: "-100".into(), chat_kind: Some(ChatKind::Group), mentioned, reply_to_bot: reply, ..dm(event, sender, text) }
}

async fn pair(rig: &Rig, sender: &str) {
    let Handled::Pairing(PairingOutcome::Issued(code)) = rig.hub.handle("ch_1", dm(9_000, sender, "hi")).await else {
        panic!("expected a pairing code");
    };
    rig.hub.resolve_pairing("ch_1", &code, true).await.expect("approve");
}

#[tokio::test]
async fn unknown_sender_gets_a_code_and_runs_after_desktop_approval() {
    let mut rig = rig(config());
    let first = rig.hub.handle("ch_1", dm(1, "42", "hello")).await;
    let Handled::Pairing(PairingOutcome::Issued(code)) = first else { panic!("{first:?}") };
    assert!(rig.channel.texts()[0].contains(&code));
    assert!(rig.runner.jobs.lock().expect("lock").is_empty(), "no task before approval");

    // Re-DM: same code, no new request.
    assert_eq!(rig.hub.handle("ch_1", dm(2, "42", "hello?")).await, Handled::Pairing(PairingOutcome::Pending(code.clone())));
    assert_eq!(rig.hub.pending_pairings("ch_1").expect("pending").len(), 1);

    let approved = rig.hub.resolve_pairing("ch_1", &code, true).await.expect("approve");
    assert_eq!(approved.sender_id, "42");
    assert!(rig.channel.texts().last().is_some_and(|t| t.contains("paired")));
    assert!(rig.hub.pending_pairings("ch_1").expect("pending").is_empty());

    assert!(matches!(rig.hub.handle("ch_1", dm(3, "42", "list files")).await, Handled::Queued(_)));
    assert_eq!(rig.done.recv().await.as_deref(), Some("list files"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(rig.channel.texts().last().map(String::as_str), Some("echo: list files"));
    let job = rig.runner.jobs.lock().expect("lock")[0].clone();
    assert_eq!(job.session_key, "agent:main:telegram:direct:42");
    assert!(job.prompt.contains("EXTERNAL_UNTRUSTED_CONTENT"));
    assert_eq!(job.origin, "Telegram · @42");
    assert_eq!(job.workspace_id, "ws_1");
}

#[tokio::test]
async fn pairing_codes_expire_and_cap_at_three() {
    let rig = rig(config());
    for (i, sender) in ["a", "b", "c"].iter().enumerate() {
        assert!(matches!(rig.hub.handle("ch_1", dm(i as u32, sender, "x")).await, Handled::Pairing(PairingOutcome::Issued(_))));
    }
    assert_eq!(rig.hub.handle("ch_1", dm(10, "d", "x")).await, Handled::Pairing(PairingOutcome::Full));
    let code = rig.hub.pending_pairings("ch_1").expect("p")[0].code.clone();
    rig.clock.fetch_add(3_600, Ordering::SeqCst);
    assert!(rig.hub.pending_pairings("ch_1").expect("p").is_empty());
    assert!(rig.hub.resolve_pairing("ch_1", &code, true).await.is_err(), "expired code cannot be approved");
    assert!(rig.state.allow_list("ch_1").expect("allow").is_empty());
}

#[tokio::test]
async fn denied_pairing_does_not_allow() {
    let rig = rig(config());
    let Handled::Pairing(PairingOutcome::Issued(code)) = rig.hub.handle("ch_1", dm(1, "42", "x")).await else { panic!() };
    rig.hub.resolve_pairing("ch_1", &code, false).await.expect("deny");
    assert!(rig.state.allow_list("ch_1").expect("allow").is_empty());
    // Asking again issues a new code instead of running.
    assert!(matches!(rig.hub.handle("ch_1", dm(2, "42", "x")).await, Handled::Pairing(PairingOutcome::Issued(_))));
}

#[tokio::test]
async fn allowlist_and_disabled_policies_drop_silently() {
    let rig = rig(ChannelConfig { dm_policy: DmPolicy::Allowlist, ..config() });
    assert!(matches!(rig.hub.handle("ch_1", dm(1, "42", "x")).await, Handled::Dropped(_)));
    assert!(rig.channel.sent().is_empty());
    let rig = rig_with(DmPolicy::Disabled);
    assert!(matches!(rig.hub.handle("ch_1", dm(1, "42", "x")).await, Handled::Dropped(_)));
    assert!(rig.channel.sent().is_empty());
}

fn rig_with(policy: DmPolicy) -> Rig {
    rig(ChannelConfig { dm_policy: policy, ..config() })
}

#[tokio::test]
async fn duplicate_inbound_events_are_handled_once() {
    let rig = rig(config());
    pair(&rig, "42").await;
    assert!(matches!(rig.hub.handle("ch_1", dm(1, "42", "x")).await, Handled::Queued(_)));
    assert_eq!(rig.hub.handle("ch_1", dm(1, "42", "x")).await, Handled::Duplicate);
}

#[tokio::test]
async fn redelivered_reply_is_not_sent_twice() {
    let rig = rig(config());
    let message = dm(1, "42", "x");
    let channel: &dyn Channel = rig.channel.as_ref();
    let sent = next_agent_channels::outbound::send_reply(channel, rig.state.as_ref(), &message, "answer").await.expect("send");
    assert_eq!(sent, 1);
    let again = next_agent_channels::outbound::send_reply(channel, rig.state.as_ref(), &message, "answer").await.expect("send");
    assert_eq!(again, 0);
    assert_eq!(rig.channel.texts(), ["answer"]);
}

#[tokio::test]
async fn long_replies_are_chunked() {
    let rig = rig(config());
    let channel = Arc::new(FakeChannel { config: config(), sent: Mutex::default(), next_id: AtomicU64::new(1), limit: 100 });
    let message = dm(1, "42", "x");
    let text = "word ".repeat(100);
    let sent = next_agent_channels::outbound::send_reply(channel.as_ref(), rig.state.as_ref(), &message, &text).await.expect("send");
    assert!(sent >= 5);
    assert!(channel.texts().iter().all(|t| t.encode_utf16().count() <= 100));
}

#[tokio::test]
async fn group_gating_requires_policy_sender_and_mention() {
    let rig = rig(ChannelConfig { group_policy: GroupPolicy::Allowlist, groups: vec!["-100".into()], ..config() });
    pair(&rig, "42").await;
    let before = rig.channel.sent().len();
    // Allowed sender, not mentioned: ignored.
    assert_eq!(rig.hub.handle("ch_1", group(1, "42", "hey", false, false)).await, Handled::Dropped("not mentioned"));
    // Unknown sender replying to the bot: still ignored.
    assert_eq!(rig.hub.handle("ch_1", group(2, "7", "hey", false, true)).await, Handled::Dropped("sender is not on the allow-list"));
    // Unknown sender mentioning the bot: ignored, no pairing in groups.
    assert_eq!(rig.hub.handle("ch_1", group(3, "7", "@bot hey", true, false)).await, Handled::Dropped("sender is not on the allow-list"));
    assert_eq!(rig.channel.sent().len(), before, "nothing is sent for ignored group messages");
    // Allowed sender replying to the bot: runs, in the group's own session.
    let Handled::Queued(session) = rig.hub.handle("ch_1", group(4, "42", "go", false, true)).await else { panic!() };
    assert_eq!(session, "agent:main:telegram:group:-100");
}

#[tokio::test]
async fn same_session_is_ordered_and_new_resets_history() {
    let mut rig = rig(config());
    pair(&rig, "42").await;
    *rig.runner.delay.lock().expect("lock") = Duration::from_millis(30);
    rig.hub.handle("ch_1", dm(1, "42", "first")).await;
    rig.hub.handle("ch_1", dm(2, "42", "second")).await;
    assert_eq!(rig.done.recv().await.as_deref(), Some("first"));
    assert_eq!(rig.done.recv().await.as_deref(), Some("second"));
    let second = rig.runner.jobs.lock().expect("lock")[1].prompt.clone();
    assert!(second.contains("Earlier in this conversation"), "history is replayed");
    assert!(second.contains("You answered: echo: first"));

    assert_eq!(rig.hub.handle("ch_1", dm(3, "42", "/new")).await, Handled::Command("new"));
    rig.hub.handle("ch_1", dm(4, "42", "third")).await;
    assert_eq!(rig.done.recv().await.as_deref(), Some("third"));
    let third = rig.runner.jobs.lock().expect("lock")[2].prompt.clone();
    assert!(!third.contains("Earlier in this conversation"));
}

#[tokio::test]
async fn stop_cancels_the_running_task() {
    let mut rig = rig(config());
    pair(&rig, "42").await;
    *rig.runner.delay.lock().expect("lock") = Duration::from_secs(60);
    rig.hub.handle("ch_1", dm(1, "42", "long job")).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(rig.hub.handle("ch_1", dm(2, "42", "/stop")).await, Handled::Command("stop"));
    assert_eq!(rig.done.recv().await.as_deref(), Some("long job"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let texts = rig.channel.texts();
    assert!(texts.contains(&"Stopped.".to_string()), "{texts:?}");
}

#[tokio::test]
async fn commands_need_access_first() {
    let rig = rig(ChannelConfig { dm_policy: DmPolicy::Allowlist, ..config() });
    assert!(matches!(rig.hub.handle("ch_1", dm(1, "7", "/stop")).await, Handled::Dropped(_)));
}

#[tokio::test]
async fn media_gets_a_clear_not_supported_reply() {
    let rig = rig(config());
    pair(&rig, "42").await;
    let mut photo = dm(1, "42", "");
    photo.media = Some("photo".into());
    assert_eq!(rig.hub.handle("ch_1", photo).await, Handled::Unsupported);
    assert!(rig.channel.texts().last().is_some_and(|t| t.contains("not supported yet")));
}

#[tokio::test]
async fn approval_wait_is_announced_once_and_previews_stream_by_edit() {
    let mut rig = rig(config());
    pair(&rig, "42").await;
    *rig.runner.progress.lock().expect("lock") = vec![
        TaskProgress::Delta("Work".into()),
        TaskProgress::WaitingForApproval { approval_id: "apr_1".into() },
        TaskProgress::WaitingForApproval { approval_id: "apr_1".into() },
        TaskProgress::Delta("ing".into()),
    ];
    let before = rig.channel.sent().len();
    rig.hub.handle("ch_1", dm(1, "42", "do it")).await;
    rig.done.recv().await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let sent: Vec<Sent> = rig.channel.sent()[before..].to_vec();
    let waits = sent.iter().filter(|s| matches!(s, Sent::Send { text, .. } if text.contains("Waiting for approval on the desktop"))).count();
    assert_eq!(waits, 1, "{sent:?}");
    // The preview message was created, then edited into the final answer.
    let Some(Sent::Send { text: first, .. }) = sent.first() else { panic!("{sent:?}") };
    assert!(first.starts_with("Work"));
    assert!(matches!(sent.last(), Some(Sent::Edit { text, .. }) if text == "echo: do it"), "{sent:?}");
    // No approve/deny buttons or commands are ever offered in chat.
    assert!(rig.channel.texts().iter().all(|t| !t.to_lowercase().contains("/approve")));
}

#[tokio::test]
async fn deliver_only_reaches_paired_chats_and_is_idempotent() {
    let rig = rig(config());
    let delivery = Delivery { channel_id: "ch_1".into(), chat_id: "42".into(), text: "daily report".into(), idempotency_key: Some("job-1:2026-11-14".into()) };
    assert!(rig.hub.deliver(&delivery).await.is_err(), "unpaired chat");
    pair(&rig, "42").await;
    assert_eq!(rig.hub.deliver(&delivery).await, Ok(1));
    assert_eq!(rig.hub.deliver(&delivery).await, Ok(0), "same key, delivered once");
    let unknown = Delivery { channel_id: "nope".into(), ..delivery.clone() };
    assert!(rig.hub.deliver(&unknown).await.is_err());
    assert!(matches!(rig.channel.sent().last(), Some(Sent::Send { chat, text, .. }) if chat == "42" && text == "daily report"));
}

#[tokio::test]
async fn group_replies_thread_to_the_message() {
    let mut rig = rig(ChannelConfig { group_policy: GroupPolicy::Open, require_mention: false, ..config() });
    pair(&rig, "42").await;
    rig.hub.handle("ch_1", group(1, "42", "hi", false, false)).await;
    rig.done.recv().await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(rig.channel.sent().last(), Some(Sent::Send { reply_to: Some(id), chat, .. }) if id == "m1" && chat == "-100"));
}
