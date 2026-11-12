//! Policy evaluation for agent tool requests.
//!
//! Pure and I/O-free: [`evaluate`] takes a request plus context and returns a
//! [`Decision`]. Filesystem facts (symlinks, junctions) are injected through
//! [`FsFacts`] via [`explain_with`]. The same decision table is mirrored in
//! TypeScript under `packages/policy` and both are checked against the shared
//! fixtures in `fixtures/` (`policy-cases.json`, `command-corpus.json`,
//! `path-corpus.json`), so the Rust engine and the UI preview cannot drift.
//!
//! See `docs/architecture/Permission Engine.md` and
//! `docs/architecture/Module Boundaries.md`.

pub mod command;
pub mod decision;
pub mod engine;
pub mod mode;
pub mod path;
pub mod rules;

pub use command::{analyze, classify, Analysis, CommandClass};
pub use decision::Decision;
pub use engine::{always_rule, evaluate, explain, explain_with, Context, Evaluation, Request};
pub use mode::{BypassGrant, Mode, ModeError, Origin, Transition};
pub use path::{FsFacts, Lexical, MapFs, PathStyle, Sandbox, Zone};
pub use rules::{Effect, Rule, RuleSet, RuleTarget, Scope};

/// Idle time after which a `bypassPermissions` session reverts to `manual`
/// (see [`Mode::after_idle`]).
pub const BYPASS_IDLE_TIMEOUT_SECONDS: u64 = 30 * 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_cycle_stays_within_everyday_modes() {
        let mut mode = Mode::default();
        assert_eq!(mode, Mode::Manual);
        for _ in 0..6 {
            mode = mode.cycle_next();
            assert_ne!(mode, Mode::BypassPermissions);
        }
        assert_eq!(mode, Mode::Manual);
    }

    #[test]
    fn bypass_requires_explicit_construction() {
        assert!(Mode::from_cli_flag("--dangerously-skip-permissions").is_some());
        assert!(Mode::from_cli_flag("--auto").is_none());
    }
}
