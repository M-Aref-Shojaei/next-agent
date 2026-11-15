// Ported from OmniRoute (MIT, © 2026 diegosouzapw): src/lib/resilience/adaptiveCircuit.ts @ v3.8.53
//! Per-provider circuit breaker: closed → open → half-open.
//!
//! - **Closed**: requests flow; [`BreakerPolicy::threshold`] consecutive
//!   provider failures (transport errors, 5xx) open it.
//! - **Open**: requests are refused until the cooldown ends. The cooldown
//!   starts at [`BreakerPolicy::base`] and doubles on each re-trip, up to
//!   [`BreakerPolicy::max`] (upstream uses a fixed 60 s).
//! - **Half-open**: exactly one probe may be in flight (upstream allows
//!   several). Its success closes the breaker; its failure re-opens it. A
//!   probe that never reports back is considered lost after one cooldown.
//!
//! The routing score (closed 1.0, half-open 0.5, open 0) moves healthy
//! providers ahead in combos and feeds the status-bar dot.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerPolicy {
    pub threshold: u32,
    pub base: Duration,
    pub max: Duration,
}

impl Default for BreakerPolicy {
    fn default() -> Self {
        Self {
            threshold: 3,
            base: Duration::from_secs(60),
            max: Duration::from_mins(30),
        }
    }
}

/// What the breaker allows right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permit {
    Allowed,
    /// The single half-open probe: report its result.
    Probe,
    Refused,
}

/// Public view of one breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

impl BreakerState {
    #[must_use]
    pub fn score(self) -> f64 {
        match self {
            Self::Closed => 1.0,
            Self::HalfOpen => 0.5,
            Self::Open => 0.0,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half-open",
        }
    }
}

#[derive(Debug, Default)]
struct Entry {
    failures: u32,
    trips: u32,
    open_until: Option<Instant>,
    probe_started: Option<Instant>,
}

/// Exponential backoff `base·2^(level−1)`, capped at `max`. Level 0 and 1 both give `base`.
pub(crate) fn backoff(base: Duration, max: Duration, level: u32) -> Duration {
    let factor = 2u32.saturating_pow(level.saturating_sub(1).min(31));
    base.saturating_mul(factor).min(max)
}

/// All providers' breakers.
#[derive(Debug, Default)]
pub struct Breakers {
    pub policy: BreakerPolicy,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Breakers {
    #[must_use]
    pub fn new(policy: BreakerPolicy) -> Self {
        Self {
            policy,
            entries: Mutex::default(),
        }
    }

    fn cooldown(&self, trips: u32) -> Duration {
        backoff(self.policy.base, self.policy.max, trips)
    }

    fn state_of(entry: &Entry, now: Instant) -> BreakerState {
        match entry.open_until {
            None => BreakerState::Closed,
            Some(until) if until > now => BreakerState::Open,
            Some(_) => BreakerState::HalfOpen,
        }
    }

    #[must_use]
    pub fn state(&self, provider: &str) -> BreakerState {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .get(provider)
            .map_or(BreakerState::Closed, |e| Self::state_of(e, Instant::now()))
    }

    /// Ask to send a request to `provider`.
    pub fn permit(&self, provider: &str) -> Permit {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = entries.get_mut(provider) else {
            return Permit::Allowed;
        };
        match Self::state_of(entry, now) {
            BreakerState::Closed => Permit::Allowed,
            BreakerState::Open => Permit::Refused,
            BreakerState::HalfOpen => {
                let lost = self.cooldown(entry.trips);
                let busy = entry
                    .probe_started
                    .is_some_and(|started| now.duration_since(started) < lost);
                if busy {
                    Permit::Refused
                } else {
                    entry.probe_started = Some(now);
                    Permit::Probe
                }
            }
        }
    }

    /// The request ended without telling us about the provider's health
    /// (rate limit, missing key): free the probe slot.
    pub fn release(&self, provider: &str) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get_mut(provider) {
            entry.probe_started = None;
        }
    }

    /// Any success closes the breaker.
    pub fn success(&self, provider: &str) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.remove(provider);
    }

    /// A provider failure; returns the new state.
    pub fn failure(&self, provider: &str) -> BreakerState {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.entry(provider.to_string()).or_default();
        let half_open = Self::state_of(entry, now) == BreakerState::HalfOpen;
        entry.failures = entry.failures.saturating_add(1);
        if half_open || entry.failures >= self.policy.threshold {
            entry.trips = entry.trips.saturating_add(1);
            entry.open_until = Some(now + self.cooldown(entry.trips));
            entry.probe_started = None;
            entry.failures = 0;
            tracing::warn!(target: "router", %provider, trips = entry.trips, "circuit breaker opened");
        }
        Self::state_of(entry, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breakers(base_ms: u64) -> Breakers {
        Breakers::new(BreakerPolicy {
            threshold: 3,
            base: Duration::from_millis(base_ms),
            max: Duration::from_millis(base_ms * 3),
        })
    }

    #[test]
    fn opens_after_three_failures_and_probes_once() {
        let b = breakers(40);
        assert_eq!(b.permit("p"), Permit::Allowed);
        assert_eq!(b.failure("p"), BreakerState::Closed);
        assert_eq!(b.failure("p"), BreakerState::Closed);
        assert_eq!(b.failure("p"), BreakerState::Open);
        assert_eq!(b.permit("p"), Permit::Refused);
        assert_eq!(b.state("p").score(), 0.0);
        assert_eq!(b.state("other"), BreakerState::Closed, "per provider");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(b.state("p"), BreakerState::HalfOpen);
        assert_eq!(b.permit("p"), Permit::Probe);
        assert_eq!(b.permit("p"), Permit::Refused, "one probe in flight");
        b.success("p");
        assert_eq!(b.state("p"), BreakerState::Closed);
        assert_eq!(b.permit("p"), Permit::Allowed);
    }

    #[test]
    fn failed_probe_reopens_with_a_longer_cooldown_up_to_the_cap() {
        let b = breakers(30);
        for _ in 0..3 {
            b.failure("p");
        }
        assert_eq!(b.cooldown(1), Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(b.permit("p"), Permit::Probe);
        assert_eq!(
            b.failure("p"),
            BreakerState::Open,
            "half-open failure re-trips"
        );
        // Second trip: 60 ms, so still open after 40 ms.
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(b.state("p"), BreakerState::Open);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(b.state("p"), BreakerState::HalfOpen);
        assert_eq!(b.cooldown(2), Duration::from_millis(60));
        assert_eq!(b.cooldown(9), Duration::from_millis(90), "capped");
    }

    #[test]
    fn a_lost_probe_is_released_after_one_cooldown() {
        let b = breakers(20);
        for _ in 0..3 {
            b.failure("p");
        }
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(b.permit("p"), Permit::Probe);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(b.permit("p"), Permit::Probe, "first probe never reported");
    }
}
