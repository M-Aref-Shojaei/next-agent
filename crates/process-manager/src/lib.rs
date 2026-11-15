//! Sidecar lifecycle and health checks.
//!
//! M1 only needs probe-only supervision of an external endpoint (the user-run
//! router), so this crate ships [`HealthMonitor`]: it polls a [`HealthProbe`]
//! on an interval (and on demand) and publishes the latest [`HealthReport`]
//! through a `tokio::sync::watch` channel the UI layer can forward.
//!
//! The crate knows nothing about HTTP; probes are supplied by their adapters
//! (the router client implements [`HealthProbe`] in `next-agent-core`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;

/// Lifecycle state of a managed or probed service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Healthy,
    Degraded,
    Crashed,
}

impl ServiceState {
    /// Whether the service can serve requests right now.
    #[must_use]
    pub fn is_available(self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }
}

/// Coarse health of an endpoint, as shown by the status-bar dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthStatus {
    /// Answered correctly and quickly.
    Ok,
    /// Reachable but slow, unauthorized or answering unexpectedly.
    Degraded,
    /// Connection refused, DNS failure or timeout.
    Down,
}

impl HealthStatus {
    /// Wire label (`ok | degraded | down`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Down => "down",
        }
    }

    /// Lifecycle state for a probe-only (unmanaged) service.
    #[must_use]
    pub fn service_state(self) -> ServiceState {
        match self {
            Self::Ok => ServiceState::Healthy,
            Self::Degraded => ServiceState::Degraded,
            Self::Down => ServiceState::Crashed,
        }
    }
}

/// Result of one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub status: HealthStatus,
    /// Human-readable explanation (e.g. "401 unauthorized", "3 models").
    pub detail: String,
    /// Round-trip time of the probe, if it got that far.
    pub latency_ms: Option<u64>,
    /// How many probes have completed, starting at 1 (0 = not yet probed).
    pub checks: u64,
}

impl HealthReport {
    #[must_use]
    pub fn new(status: HealthStatus, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
            latency_ms: None,
            checks: 0,
        }
    }

    #[must_use]
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency_ms = Some(u64::try_from(latency.as_millis()).unwrap_or(u64::MAX));
        self
    }

    /// Placeholder before the first probe. Its `Down` status is only a default:
    /// callers must treat `checks == 0` as unknown, not down.
    fn pending() -> Self {
        Self::new(HealthStatus::Down, "not checked yet")
    }
}

/// Boxed future returned by [`HealthProbe::probe`].
pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = HealthReport> + Send + 'a>>;

/// Something that can be health-checked. Implementations enforce their own
/// timeout; the monitor additionally caps each probe at `timeout`.
pub trait HealthProbe: Send + Sync {
    fn probe(&self) -> ProbeFuture<'_>;
}

/// Polling configuration.
#[derive(Debug, Clone, Copy)]
pub struct MonitorConfig {
    /// Delay between probes.
    pub interval: Duration,
    /// Hard cap per probe; exceeding it reports `down`.
    pub timeout: Duration,
}

impl Default for MonitorConfig {
    /// 2 s interval + 3 s timeout keeps status changes visible within the
    /// M1 acceptance bound of 5 s.
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(2),
            timeout: Duration::from_secs(3),
        }
    }
}

/// Background poller for one probe. Dropping it stops polling.
#[derive(Debug)]
pub struct HealthMonitor {
    status: watch::Receiver<HealthReport>,
    refresh: Arc<Notify>,
    task: JoinHandle<()>,
}

impl HealthMonitor {
    /// Start polling on the current tokio runtime.
    ///
    /// # Panics
    /// Panics if called outside a tokio runtime.
    #[must_use]
    pub fn spawn(probe: Arc<dyn HealthProbe>, config: MonitorConfig) -> Self {
        let (sender, status) = watch::channel(HealthReport::pending());
        let refresh = Arc::new(Notify::new());
        let wake = Arc::clone(&refresh);
        let task = tokio::spawn(async move {
            let mut checks = 0_u64;
            loop {
                let mut report = match tokio::time::timeout(config.timeout, probe.probe()).await {
                    Ok(report) => report,
                    Err(_) => HealthReport::new(HealthStatus::Down, "probe timed out"),
                };
                checks += 1;
                report.checks = checks;
                if sender.send(report).is_err() {
                    return;
                }
                tokio::select! {
                    () = tokio::time::sleep(config.interval) => {}
                    () = wake.notified() => {}
                }
            }
        });
        Self {
            status,
            refresh,
            task,
        }
    }

    /// Latest report.
    #[must_use]
    pub fn current(&self) -> HealthReport {
        self.status.borrow().clone()
    }

    /// Receiver that wakes on every new report (for forwarding to the UI).
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<HealthReport> {
        self.status.clone()
    }

    /// Probe again now instead of waiting for the interval.
    pub fn refresh(&self) {
        self.refresh.notify_one();
    }
}

impl Drop for HealthMonitor {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[test]
    fn availability() {
        assert!(ServiceState::Healthy.is_available());
        assert!(ServiceState::Degraded.is_available());
        assert!(!ServiceState::Crashed.is_available());
        assert!(!ServiceState::Stopped.is_available());
    }

    #[test]
    fn status_labels_and_states() {
        assert_eq!(HealthStatus::Ok.as_str(), "ok");
        assert_eq!(HealthStatus::Degraded.as_str(), "degraded");
        assert_eq!(HealthStatus::Down.as_str(), "down");
        assert_eq!(HealthStatus::Down.service_state(), ServiceState::Crashed);
        assert!(HealthStatus::Degraded.service_state().is_available());
    }

    /// Replays a list of statuses, repeating the last one.
    struct Scripted {
        script: Mutex<Vec<HealthStatus>>,
        calls: AtomicUsize,
    }

    impl HealthProbe for Scripted {
        fn probe(&self) -> ProbeFuture<'_> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let mut script = self.script.lock().expect("lock");
                let status = if script.len() > 1 {
                    script.remove(0)
                } else {
                    script[0]
                };
                HealthReport::new(status, "scripted")
            })
        }
    }

    struct Hangs;
    impl HealthProbe for Hangs {
        fn probe(&self) -> ProbeFuture<'_> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                HealthReport::new(HealthStatus::Ok, "never")
            })
        }
    }

    async fn next(rx: &mut watch::Receiver<HealthReport>) -> HealthReport {
        rx.changed().await.expect("monitor alive");
        rx.borrow().clone()
    }

    #[tokio::test(start_paused = true)]
    async fn polls_on_interval_and_publishes_transitions() {
        let probe = Arc::new(Scripted {
            script: Mutex::new(vec![HealthStatus::Ok, HealthStatus::Down, HealthStatus::Ok]),
            calls: AtomicUsize::new(0),
        });
        let monitor = HealthMonitor::spawn(probe.clone(), MonitorConfig::default());
        let mut rx = monitor.subscribe();
        let first = next(&mut rx).await;
        assert_eq!((first.status, first.checks), (HealthStatus::Ok, 1));
        assert_eq!(next(&mut rx).await.status, HealthStatus::Down);
        assert_eq!(next(&mut rx).await.status, HealthStatus::Ok);
        assert_eq!(monitor.current().status, HealthStatus::Ok);
        assert!(probe.calls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test(start_paused = true)]
    async fn hung_probe_reports_down_after_timeout() {
        let monitor = HealthMonitor::spawn(Arc::new(Hangs), MonitorConfig::default());
        let mut rx = monitor.subscribe();
        let report = next(&mut rx).await;
        assert_eq!(report.status, HealthStatus::Down);
        assert_eq!(report.detail, "probe timed out");
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_probes_before_the_interval() {
        let probe = Arc::new(Scripted {
            script: Mutex::new(vec![HealthStatus::Ok]),
            calls: AtomicUsize::new(0),
        });
        let config = MonitorConfig {
            interval: Duration::from_secs(3600),
            timeout: Duration::from_secs(3),
        };
        let monitor = HealthMonitor::spawn(probe.clone(), config);
        let mut rx = monitor.subscribe();
        let _ = next(&mut rx).await;
        monitor.refresh();
        let report = tokio::time::timeout(Duration::from_secs(5), next(&mut rx))
            .await
            .expect("refresh should trigger a probe well before the interval");
        assert_eq!(report.checks, 2);
    }

    #[test]
    fn latency_is_recorded_in_millis() {
        let report =
            HealthReport::new(HealthStatus::Ok, "x").with_latency(Duration::from_millis(42));
        assert_eq!(report.latency_ms, Some(42));
    }
}
