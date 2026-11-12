//! Task identifiers and lifecycle status.

use std::fmt;

/// Prefix-stable task id (`tsk_` + 16 hex chars).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(String);

impl TaskId {
    /// Build an id from a caller-supplied unique seed (e.g. timestamp + nonce).
    #[must_use]
    pub fn from_seed(seed: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in seed.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(format!("tsk_{hash:016x}"))
    }

    /// Parse an already-formatted id (e.g. from the database).
    #[must_use]
    pub fn parse(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        let hex = value.strip_prefix("tsk_")?;
        (hex.len() == 16 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Persisted form, matching the SQL CHECK constraint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse the persisted form.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_deterministic_and_parseable() {
        let id = TaskId::from_seed("2026-11-10:abc");
        assert_eq!(id, TaskId::from_seed("2026-11-10:abc"));
        assert_ne!(id, TaskId::from_seed("2026-11-10:abd"));
        assert_eq!(TaskId::parse(id.as_str()), Some(id));
    }

    #[test]
    fn ids_reject_malformed_values() {
        assert_eq!(TaskId::parse("tsk_"), None);
        assert_eq!(TaskId::parse("tsk_xyz"), None);
        assert_eq!(TaskId::parse("ws_0123456789abcdef"), None);
        assert_eq!(TaskId::parse(format!("tsk_{}", "g".repeat(16))), None);
    }

    #[test]
    fn status_round_trip() {
        for status in [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            assert_eq!(TaskStatus::parse(status.as_str()), Some(status));
        }
        assert!(TaskStatus::Completed.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert_eq!(TaskStatus::parse("done"), None);
    }
}
