//! Result of evaluating a tool request.

/// Outcome of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Execute without prompting.
    Allow {
        /// Human-readable justification, recorded in the event log.
        reason: String,
    },
    /// Pause and surface an approval card.
    Ask {
        /// Why the user is being asked.
        reason: String,
    },
    /// Refuse outright; no prompt can override.
    Deny {
        /// Why the action is blocked.
        reason: String,
    },
}

impl Decision {
    #[must_use]
    pub fn allow(reason: impl Into<String>) -> Self {
        Self::Allow {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn ask(reason: impl Into<String>) -> Self {
        Self::Ask {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Allow { reason } | Self::Ask { reason } | Self::Deny { reason } => reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates() {
        assert!(Decision::allow("safe").is_allowed());
        assert!(!Decision::ask("x").is_allowed());
        assert!(Decision::deny("blocked").is_denied());
        assert_eq!(Decision::deny("blocked").reason(), "blocked");
    }
}
