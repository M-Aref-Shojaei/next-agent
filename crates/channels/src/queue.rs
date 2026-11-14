//! Per-session ordered queues (OpenClaw's behaviour, see
//! `docs/upstream/OpenClaw.md`; no upstream code). Jobs with the same
//! session key run one at a time, in arrival order; different sessions run concurrently. `/stop`
//! cancels the running job and drops the ones waiting behind it.

use crate::channel::BoxFuture;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{mpsc, watch};

/// Cancellation signal handed to a running job.
#[derive(Debug, Clone)]
pub struct StopToken(watch::Receiver<bool>);

impl StopToken {
    /// A token that never fires (tests, one-off deliveries).
    #[must_use]
    pub fn never() -> Self {
        let (tx, rx) = watch::channel(false);
        // Keep the sender alive so `stopped` waits forever.
        std::mem::forget(tx);
        Self(rx)
    }

    #[must_use]
    pub fn is_stopped(&self) -> bool {
        *self.0.borrow()
    }

    /// Resolves once stop is requested.
    pub async fn stopped(&mut self) {
        while !*self.0.borrow_and_update() {
            if self.0.changed().await.is_err() {
                // Sender gone without a stop: never resolves.
                std::future::pending::<()>().await;
            }
        }
    }
}

type Handler<J> = Arc<dyn Fn(J, StopToken) -> BoxFuture<'static, ()> + Send + Sync>;

struct Lane<J> {
    jobs: mpsc::UnboundedSender<(u64, J)>,
    /// Stop signal of the job now running (replaced per job).
    current: Arc<Mutex<Option<watch::Sender<bool>>>>,
    /// Jobs with an epoch below this are dropped unrun (`/stop`).
    floor: Arc<Mutex<u64>>,
    next_epoch: u64,
}

/// Ordered lanes keyed by session.
pub struct SessionQueues<J: Send + 'static> {
    lanes: Mutex<HashMap<String, Lane<J>>>,
    handler: Handler<J>,
}

impl<J: Send + 'static> SessionQueues<J> {
    #[must_use]
    pub fn new(handler: impl Fn(J, StopToken) -> BoxFuture<'static, ()> + Send + Sync + 'static) -> Self {
        Self {
            lanes: Mutex::new(HashMap::new()),
            handler: Arc::new(handler),
        }
    }

    /// Queue `job` behind the session's earlier jobs. Must be called inside
    /// a Tokio runtime.
    pub fn push(&self, session: &str, job: J) {
        let mut lanes = self.lanes.lock().unwrap_or_else(PoisonError::into_inner);
        let lane = lanes
            .entry(session.to_string())
            .or_insert_with(|| self.spawn_lane());
        lane.next_epoch += 1;
        let epoch = lane.next_epoch;
        if lane.jobs.send((epoch, job)).is_err() {
            // The worker ended (runtime shutdown); nothing to do.
        }
    }

    /// Cancel the running job of `session` and drop the queued ones.
    /// Returns whether a job was running.
    pub fn stop(&self, session: &str) -> bool {
        let lanes = self.lanes.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(lane) = lanes.get(session) else {
            return false;
        };
        *lane.floor.lock().unwrap_or_else(PoisonError::into_inner) = lane.next_epoch + 1;
        let current = lane
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        current.is_some_and(|stop| stop.send(true).is_ok())
    }

    fn spawn_lane(&self) -> Lane<J> {
        let (jobs, mut rx) = mpsc::unbounded_channel::<(u64, J)>();
        let current: Arc<Mutex<Option<watch::Sender<bool>>>> = Arc::default();
        let floor = Arc::new(Mutex::new(0u64));
        let handler = Arc::clone(&self.handler);
        let worker_current = Arc::clone(&current);
        let worker_floor = Arc::clone(&floor);
        tokio::spawn(async move {
            while let Some((epoch, job)) = rx.recv().await {
                if epoch < *worker_floor.lock().unwrap_or_else(PoisonError::into_inner) {
                    continue;
                }
                let (stop, token) = watch::channel(false);
                *worker_current.lock().unwrap_or_else(PoisonError::into_inner) = Some(stop);
                handler(job, StopToken(token)).await;
                worker_current
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
            }
        });
        Lane {
            jobs,
            current,
            floor,
            next_epoch: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::mpsc::UnboundedSender;

    type Log = UnboundedSender<String>;

    fn queues(log: Log, delay_ms: u64) -> SessionQueues<String> {
        SessionQueues::new(move |job: String, mut stop: StopToken| {
            let log = log.clone();
            Box::pin(async move {
                let _ = log.send(format!("start {job}"));
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(delay_ms)) => {
                        let _ = log.send(format!("end {job}"));
                    }
                    () = stop.stopped() => {
                        let _ = log.send(format!("stopped {job}"));
                    }
                }
            })
        })
    }

    async fn take(rx: &mut mpsc::UnboundedReceiver<String>, n: usize) -> Vec<String> {
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(rx.recv().await.expect("log"));
        }
        out
    }

    #[tokio::test(start_paused = true)]
    async fn same_session_runs_in_order() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let q = queues(tx, 50);
        q.push("s", "a".into());
        q.push("s", "b".into());
        assert_eq!(take(&mut rx, 4).await, ["start a", "end a", "start b", "end b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn different_sessions_run_concurrently() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let q = queues(tx, 50);
        q.push("s1", "a".into());
        q.push("s2", "b".into());
        let first_two = take(&mut rx, 2).await;
        assert_eq!(first_two, ["start a", "start b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_cancels_current_and_drops_queued() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let q = queues(tx, 10_000);
        q.push("s", "a".into());
        q.push("s", "b".into());
        assert_eq!(take(&mut rx, 1).await, ["start a"]);
        assert!(q.stop("s"));
        assert_eq!(take(&mut rx, 1).await, ["stopped a"]);
        // `b` was dropped; a new job still runs.
        q.push("s", "c".into());
        assert_eq!(take(&mut rx, 1).await, ["start c"]);
        assert!(!q.stop("unknown"));
    }

    #[tokio::test]
    async fn never_token_does_not_fire() {
        let mut token = StopToken::never();
        assert!(!token.is_stopped());
        let fired = tokio::time::timeout(Duration::from_millis(20), token.stopped()).await;
        assert!(fired.is_err());
    }
}
