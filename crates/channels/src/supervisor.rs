//! Keeps a channel receiving: restarts it with exponential backoff after a
//! transient failure, stops on a fatal one (bad token), and publishes its
//! health on a watch channel. OpenClaw's design (see
//! `docs/upstream/OpenClaw.md`), our own code.

use crate::channel::{Channel, ChannelError};
use crate::types::Inbound;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

/// Lifecycle of a supervised channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Starting,
    Connected,
    Reconnecting,
    /// Gave up: needs the owner (e.g. invalid token).
    Failed,
    Stopped,
}

/// Health snapshot shown in Settings → Channels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub state: HealthState,
    pub detail: String,
    /// Restarts since the supervisor started.
    pub restarts: u32,
    /// Unix seconds of the last state change.
    pub since: u64,
}

impl Health {
    fn new(state: HealthState, detail: impl Into<String>, restarts: u32) -> Self {
        Self {
            state,
            detail: detail.into(),
            restarts,
            since: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }
}

/// Reconnect timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
    /// A run that stayed up this long resets the delay to `initial`.
    pub stable: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            stable: Duration::from_secs(60),
        }
    }
}

/// A running supervisor.
pub struct Supervised {
    pub health: watch::Receiver<Health>,
    shutdown: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl Supervised {
    /// Stop receiving and wait for the loop to end.
    pub async fn stop(self) {
        let _ = self.shutdown.send(true);
        let _ = self.join.await;
    }
}

/// Run `channel` until stopped, restarting it after transient errors.
#[must_use]
pub fn supervise(
    channel: Arc<dyn Channel>,
    inbound: mpsc::Sender<Inbound>,
    backoff: Backoff,
) -> Supervised {
    let (health_tx, health) = watch::channel(Health::new(HealthState::Starting, "starting", 0));
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let join = tokio::spawn(async move {
        let health_tx = Arc::new(health_tx);
        let mut delay = backoff.initial;
        let mut restarts = 0u32;
        loop {
            let started = tokio::time::Instant::now();
            let hook_tx = Arc::clone(&health_tx);
            let hook_restarts = restarts;
            let connected = Box::new(move || {
                hook_tx.send_if_modified(|h| {
                    if h.state == HealthState::Connected {
                        return false;
                    }
                    *h = Health::new(HealthState::Connected, "receiving messages", hook_restarts);
                    true
                });
            });
            let outcome = tokio::select! {
                result = channel.start(inbound.clone(), connected) => result,
                _ = shutdown_rx.changed() => {
                    let _ = health_tx.send(Health::new(HealthState::Stopped, "stopped", restarts));
                    return;
                }
            };
            let wait = match outcome {
                Ok(()) => {
                    let _ = health_tx.send(Health::new(HealthState::Stopped, "stopped", restarts));
                    return;
                }
                Err(ChannelError::Fatal(message)) => {
                    let _ = health_tx.send(Health::new(HealthState::Failed, message, restarts));
                    return;
                }
                Err(ChannelError::Transient { message, retry_after }) => {
                    if started.elapsed() >= backoff.stable {
                        delay = backoff.initial;
                    }
                    let wait = retry_after.map_or(delay, Duration::from_secs).min(backoff.max.max(delay));
                    restarts += 1;
                    let _ = health_tx.send(Health::new(
                        HealthState::Reconnecting,
                        format!("{message}; retrying in {}s", wait.as_secs().max(1)),
                        restarts,
                    ));
                    delay = (delay * 2).min(backoff.max);
                    wait
                }
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                _ = shutdown_rx.changed() => {
                    let _ = health_tx.send(Health::new(HealthState::Stopped, "stopped", restarts));
                    return;
                }
            }
        }
    });
    Supervised {
        health,
        shutdown,
        join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{BoxFuture, ConnectedHook};
    use crate::config::ChannelConfig;
    use crate::types::{BotIdentity, Capabilities, MessageRef, Outbound};
    use std::sync::Mutex;

    /// Fails with the scripted errors, then stays connected.
    struct Flaky {
        config: ChannelConfig,
        script: Mutex<Vec<ChannelError>>,
        starts: Mutex<u32>,
    }

    impl Channel for Flaky {
        fn config(&self) -> &ChannelConfig {
            &self.config
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { text_chunk_limit: 100, edit: false, typing: false, threads: false, media: false }
        }
        fn probe(&self) -> BoxFuture<'_, Result<BotIdentity, ChannelError>> {
            Box::pin(async { Ok(BotIdentity::default()) })
        }
        fn start(&self, _inbound: mpsc::Sender<Inbound>, connected: ConnectedHook) -> BoxFuture<'_, Result<(), ChannelError>> {
            *self.starts.lock().expect("lock") += 1;
            let next = self.script.lock().expect("lock").pop();
            Box::pin(async move {
                match next {
                    Some(error) => Err(error),
                    None => {
                        connected();
                        std::future::pending().await
                    }
                }
            })
        }
        fn send(&self, _m: Outbound) -> BoxFuture<'_, Result<MessageRef, ChannelError>> {
            unreachable!()
        }
        fn edit(&self, _t: MessageRef, _x: String) -> BoxFuture<'_, Result<(), ChannelError>> {
            unreachable!()
        }
        fn typing(&self, _c: String) -> BoxFuture<'_, Result<(), ChannelError>> {
            unreachable!()
        }
    }

    fn flaky(mut script: Vec<ChannelError>) -> Arc<Flaky> {
        script.reverse();
        Arc::new(Flaky { config: ChannelConfig::default(), script: Mutex::new(script), starts: Mutex::new(0) })
    }

    async fn wait_for(health: &mut watch::Receiver<Health>, state: HealthState) -> Health {
        loop {
            if health.borrow().state == state {
                return health.borrow().clone();
            }
            health.changed().await.expect("health");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reconnects_after_transient_errors_then_reports_connected() {
        let channel = flaky(vec![ChannelError::transient("net down"), ChannelError::transient("net down")]);
        let (tx, _rx) = mpsc::channel(4);
        let mut sup = supervise(channel.clone(), tx, Backoff::default());
        let health = wait_for(&mut sup.health, HealthState::Connected).await;
        assert_eq!(health.restarts, 2);
        assert_eq!(*channel.starts.lock().expect("lock"), 3);
        sup.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_error_stops_retrying() {
        let channel = flaky(vec![ChannelError::Fatal("invalid bot token".into())]);
        let (tx, _rx) = mpsc::channel(4);
        let mut sup = supervise(channel.clone(), tx, Backoff::default());
        let health = wait_for(&mut sup.health, HealthState::Failed).await;
        assert_eq!(health.detail, "invalid bot token");
        tokio::time::sleep(Duration::from_secs(600)).await;
        assert_eq!(*channel.starts.lock().expect("lock"), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_doubles_and_honours_retry_after() {
        let channel = flaky(vec![
            ChannelError::transient("a"),
            ChannelError::transient("b"),
            ChannelError::Transient { message: "429".into(), retry_after: Some(30) },
        ]);
        let (tx, _rx) = mpsc::channel(4);
        let mut sup = supervise(channel, tx, Backoff::default());
        let mut details = Vec::new();
        loop {
            sup.health.changed().await.expect("health");
            let h = sup.health.borrow().clone();
            if h.state == HealthState::Reconnecting {
                details.push(h.detail.clone());
            }
            if h.state == HealthState::Connected {
                break;
            }
        }
        assert_eq!(details, ["a; retrying in 1s", "b; retrying in 2s", "429; retrying in 30s"]);
        sup.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn stop_reports_stopped() {
        let channel = flaky(vec![]);
        let (tx, _rx) = mpsc::channel(4);
        let mut sup = supervise(channel, tx, Backoff::default());
        wait_for(&mut sup.health, HealthState::Connected).await;
        let mut health = sup.health.clone();
        sup.stop().await;
        assert_eq!(health.borrow_and_update().state, HealthState::Stopped);
    }
}
