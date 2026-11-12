//! Sidecar lifecycle and health checks.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability() {
        assert!(ServiceState::Healthy.is_available());
        assert!(ServiceState::Degraded.is_available());
        assert!(!ServiceState::Crashed.is_available());
        assert!(!ServiceState::Stopped.is_available());
    }
}
