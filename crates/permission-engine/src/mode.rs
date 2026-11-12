//! The four permission modes and their transitions.
//!
//! - `Shift+Tab` cycles `plan → manual → auto → plan` ([`Mode::cycle_next`]);
//!   cycling never enters bypass, and cycling *out of* bypass drops to the
//!   default `manual`, never straight into `auto`.
//! - [`Mode::transition`] is the only way to change mode on a user request.
//!   Entering `bypassPermissions` needs a [`BypassGrant`], which can only be
//!   minted from the typed confirmation phrase or the
//!   `--dangerously-skip-permissions` flag, and never for a channel-originated
//!   task.
//! - Persisted modes are restored with [`Mode::from_persisted`], which refuses
//!   to restore bypass.

/// Active permission mode for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Mode {
    /// Read-only exploration; mutations and commands are blocked.
    Plan,
    /// Interactive default; mutations and commands ask the user.
    #[default]
    Manual,
    /// Autonomous inside the workspace sandbox.
    Auto,
    /// Sandbox unlocked; every sensitive action is audited.
    BypassPermissions,
}

/// The CLI flag that starts a session in bypass.
pub const BYPASS_CLI_FLAG: &str = "--dangerously-skip-permissions";

/// Text the confirmation dialog asks the user to type.
pub const BYPASS_CONFIRMATION_PHRASE: &str = "bypass permissions";

/// Where a mode change request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The desktop UI (keyboard shortcut, picker, confirmation modal).
    Ui,
    /// Command-line launch flags.
    Cli,
    /// A messaging channel (chat bridge, webhook). Never trusted with bypass.
    Channel,
}

/// Proof that a human explicitly asked for bypass. Not constructible outside
/// this module except through the two checked constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BypassGrant {
    origin: Origin,
}

impl BypassGrant {
    /// The user typed the confirmation phrase into the modal (exact match,
    /// surrounding whitespace ignored).
    #[must_use]
    pub fn from_typed_confirmation(text: &str) -> Option<Self> {
        (text.trim() == BYPASS_CONFIRMATION_PHRASE).then_some(Self { origin: Origin::Ui })
    }

    /// The session was launched with `--dangerously-skip-permissions`.
    #[must_use]
    pub fn from_cli_flag(flag: &str) -> Option<Self> {
        (flag == BYPASS_CLI_FLAG).then_some(Self {
            origin: Origin::Cli,
        })
    }

    #[must_use]
    pub fn origin(self) -> Origin {
        self.origin
    }
}

/// A requested mode change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// `Shift+Tab`.
    Cycle,
    /// Pick an everyday mode directly (`plan`, `manual`, `auto`).
    Set(Mode),
    /// Enter bypass with explicit consent.
    EnterBypass(BypassGrant),
}

/// Why a mode change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeError {
    /// `Set(BypassPermissions)` without a grant.
    BypassNeedsConfirmation,
    /// Channel-originated tasks can never enter bypass.
    ChannelCannotBypass,
}

impl std::fmt::Display for ModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BypassNeedsConfirmation => {
                "bypassPermissions requires typed confirmation or --dangerously-skip-permissions"
            }
            Self::ChannelCannotBypass => "channel-originated tasks cannot enter bypassPermissions",
        })
    }
}

impl std::error::Error for ModeError {}

impl Mode {
    /// `Shift+Tab` order: `plan -> manual -> auto -> plan`.
    ///
    /// Bypass is never reachable by cycling; leaving bypass by cycling lands
    /// on the default `manual`.
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Plan | Self::BypassPermissions => Self::Manual,
            Self::Manual => Self::Auto,
            Self::Auto => Self::Plan,
        }
    }

    /// Apply a mode change requested from `origin`.
    ///
    /// # Errors
    /// Refuses bypass without a grant, and bypass for channel tasks even with
    /// one.
    pub fn transition(self, transition: Transition, origin: Origin) -> Result<Self, ModeError> {
        match transition {
            Transition::Cycle => Ok(self.cycle_next()),
            Transition::Set(Self::BypassPermissions) => Err(ModeError::BypassNeedsConfirmation),
            Transition::Set(mode) => Ok(mode),
            Transition::EnterBypass(grant) => {
                if origin == Origin::Channel || grant.origin == Origin::Channel {
                    Err(ModeError::ChannelCannotBypass)
                } else {
                    Ok(Self::BypassPermissions)
                }
            }
        }
    }

    /// Parse the CLI escape hatch. Only the explicit flag enables bypass.
    #[must_use]
    pub fn from_cli_flag(flag: &str) -> Option<Self> {
        BypassGrant::from_cli_flag(flag).map(|_| Self::BypassPermissions)
    }

    /// Parse an everyday mode name. Never yields bypass.
    #[must_use]
    pub fn parse_everyday(name: &str) -> Option<Self> {
        match name {
            "plan" => Some(Self::Plan),
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    /// Restore a mode saved with [`Mode::wire_name`]. Bypass and unknown
    /// values fall back to the default: consent does not survive a restart.
    #[must_use]
    pub fn from_persisted(name: &str) -> Self {
        Self::parse_everyday(name).unwrap_or_default()
    }

    /// Stable lower-case name for storage and fixtures.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::BypassPermissions => "bypass",
        }
    }

    /// Bypass auto-reverts to `manual` after this much idle time.
    #[must_use]
    pub fn after_idle(self, idle_seconds: u64) -> Self {
        if self == Self::BypassPermissions && idle_seconds >= crate::BYPASS_IDLE_TIMEOUT_SECONDS {
            Self::Manual
        } else {
            self
        }
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

    const ALL: [Mode; 4] = [
        Mode::Plan,
        Mode::Manual,
        Mode::Auto,
        Mode::BypassPermissions,
    ];

    #[test]
    fn cycle_order() {
        assert_eq!(Mode::Plan.cycle_next(), Mode::Manual);
        assert_eq!(Mode::Manual.cycle_next(), Mode::Auto);
        assert_eq!(Mode::Auto.cycle_next(), Mode::Plan);
        assert_eq!(Mode::BypassPermissions.cycle_next(), Mode::Manual);
    }

    #[test]
    fn set_never_enters_bypass() {
        for from in ALL {
            for origin in [Origin::Ui, Origin::Cli, Origin::Channel] {
                assert_eq!(
                    from.transition(Transition::Set(Mode::BypassPermissions), origin),
                    Err(ModeError::BypassNeedsConfirmation)
                );
                for to in [Mode::Plan, Mode::Manual, Mode::Auto] {
                    assert_eq!(from.transition(Transition::Set(to), origin), Ok(to));
                }
            }
        }
    }

    #[test]
    fn bypass_needs_a_grant_and_a_non_channel_origin() {
        assert!(BypassGrant::from_typed_confirmation("bypass").is_none());
        assert!(BypassGrant::from_typed_confirmation("BYPASS PERMISSIONS").is_none());
        assert!(BypassGrant::from_cli_flag("--auto").is_none());
        let typed = BypassGrant::from_typed_confirmation("  bypass permissions ").expect("phrase");
        let flag = BypassGrant::from_cli_flag(BYPASS_CLI_FLAG).expect("flag");
        assert_eq!(typed.origin(), Origin::Ui);
        assert_eq!(
            Mode::Manual.transition(Transition::EnterBypass(typed), Origin::Ui),
            Ok(Mode::BypassPermissions)
        );
        assert_eq!(
            Mode::Plan.transition(Transition::EnterBypass(flag), Origin::Cli),
            Ok(Mode::BypassPermissions)
        );
        assert_eq!(
            Mode::Auto.transition(Transition::EnterBypass(typed), Origin::Channel),
            Err(ModeError::ChannelCannotBypass)
        );
    }

    #[test]
    fn persisted_bypass_is_not_restored() {
        for mode in ALL {
            let restored = Mode::from_persisted(mode.wire_name());
            assert_ne!(restored, Mode::BypassPermissions);
        }
        assert_eq!(Mode::from_persisted("auto"), Mode::Auto);
        assert_eq!(Mode::from_persisted("bypassPermissions"), Mode::Manual);
        assert_eq!(Mode::from_persisted(""), Mode::Manual);
        assert!(Mode::parse_everyday("bypass").is_none());
    }

    #[test]
    fn bypass_reverts_after_idle_timeout() {
        let limit = crate::BYPASS_IDLE_TIMEOUT_SECONDS;
        assert_eq!(
            Mode::BypassPermissions.after_idle(limit - 1),
            Mode::BypassPermissions
        );
        assert_eq!(Mode::BypassPermissions.after_idle(limit), Mode::Manual);
        assert_eq!(Mode::Auto.after_idle(limit * 10), Mode::Auto);
    }

    #[test]
    fn flags_and_labels() {
        assert_eq!(
            Mode::from_cli_flag(BYPASS_CLI_FLAG),
            Some(Mode::BypassPermissions)
        );
        assert_eq!(Mode::BypassPermissions.label(), "Bypass");
        assert!(Mode::BypassPermissions.sandbox_unlocked());
        assert!(!Mode::Auto.sandbox_unlocked());
        assert!(Mode::Plan.is_read_only());
        assert_eq!(
            ModeError::ChannelCannotBypass.to_string(),
            "channel-originated tasks cannot enter bypassPermissions"
        );
    }
}
