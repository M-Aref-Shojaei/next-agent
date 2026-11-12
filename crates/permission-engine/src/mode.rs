//! The four permission modes and their transitions.

/// Active permission mode for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Read-only exploration; mutations and commands are blocked.
    Plan,
    /// Interactive default; mutations and commands ask the user.
    Manual,
    /// Autonomous inside the workspace sandbox.
    Auto,
    /// Sandbox unlocked; every sensitive action is audited.
    BypassPermissions,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Manual
    }
}

impl Mode {
    /// `Shift+Tab` order: `plan -> manual -> auto -> plan`.
    ///
    /// Bypass is never reachable by cycling.
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Plan => Self::Manual,
            Self::Manual | Self::BypassPermissions => Self::Auto,
            Self::Auto => Self::Plan,
        }
    }

    /// Parse the CLI escape hatch. Only the explicit flag enables bypass.
    #[must_use]
    pub fn from_cli_flag(flag: &str) -> Option<Self> {
        (flag == "--dangerously-skip-permissions").then_some(Self::BypassPermissions)
    }

    #[must_use]
    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Plan)
    }

    #[must_use]
    pub fn sandbox_unlocked(self) -> bool {
        matches!(self, Self::BypassPermissions)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Manual => "Manual",
            Self::Auto => "Auto",
            Self::BypassPermissions => "Bypass",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_order() {
        assert_eq!(Mode::Plan.cycle_next(), Mode::Manual);
        assert_eq!(Mode::Manual.cycle_next(), Mode::Auto);
        assert_eq!(Mode::Auto.cycle_next(), Mode::Plan);
        assert_eq!(Mode::BypassPermissions.cycle_next(), Mode::Auto);
    }

    #[test]
    fn flags_and_labels() {
        assert_eq!(
            Mode::from_cli_flag("--dangerously-skip-permissions"),
            Some(Mode::BypassPermissions)
        );
        assert_eq!(Mode::BypassPermissions.label(), "Bypass");
        assert!(Mode::BypassPermissions.sandbox_unlocked());
        assert!(!Mode::Auto.sandbox_unlocked());
        assert!(Mode::Plan.is_read_only());
    }
}
