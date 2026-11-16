//! The one field allowlist (ADR-0013, docs/architecture/Logging.md).
//!
//! A log event keeps only fields named here; any other field is dropped and
//! counted in `dropped_fields`, so a mistake loses detail instead of leaking
//! content. `scripts/check-log-fields.mjs` reads `ALLOWED` and `FORBIDDEN`
//! from this file, so keep each as one `&[&str]` array of string literals.
//!
//! Adding a name needs the coordinator's OK and must not be a forbidden name
//! (a test below enforces that).

/// Field names an event may carry. Values are still redacted.
pub const ALLOWED: &[&str] = &[
    "error",
    "kind",
    "code",
    "status",
    "duration_ms",
    "attempt",
    "count",
    "bytes",
    "tokens_in",
    "tokens_out",
    "tokens_cached",
    "cost_micros",
    "provider",
    "model",
    "combo",
    "role",
    "mode",
    "tool",
    "decision",
    "rule_kind",
    "command",
    "ok",
    "task_id",
    "approval_id",
    "channel_id",
    "channel_kind",
    "cron_id",
    "session_id",
    "window",
    "page",
    "setting",
    "version",
    "from_version",
    "to_version",
    "migration",
    "os",
    "arch",
    "level",
    "reason",
    "retries",
    "queue",
    "files",
    "exit_code",
    "signal",
    "port",
];

/// Names that can never be allowlisted, and that `make check` rejects in any
/// log call: they name user content or credentials.
pub const FORBIDDEN: &[&str] = &[
    "prompt",
    "content",
    "text",
    "body",
    "answer",
    "response",
    "message_text",
    "title",
    "name",
    "note",
    "query",
    "args",
    "input",
    "output",
    "diff",
    "path",
    "token",
    "key",
    "secret",
    "password",
    "cookie",
    "authorization",
    "sender",
    "username",
    "email",
    "phone",
];

/// Allowed string fields that hold free text (an error description, a short
/// reason). They are redacted and cut to 300 characters. Every other string
/// field must look like a token: no whitespace, at most 128 characters, or
/// its value is replaced with [`INVALID`].
pub const FREE_TEXT: &[&str] = &["error", "reason"];

/// What replaces a token-shaped field's value when it is not a token.
pub const INVALID: &str = "[invalid]";

/// Whether `name` may be written.
#[must_use]
pub fn is_allowed(name: &str) -> bool {
    ALLOWED.contains(&name)
}

#[must_use]
pub(crate) fn is_free_text(name: &str) -> bool {
    FREE_TEXT.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_forbidden_name_is_allowlisted() {
        for name in FORBIDDEN {
            assert!(!is_allowed(name), "{name} is forbidden but allowlisted");
        }
    }

    #[test]
    fn free_text_fields_are_allowlisted() {
        for name in FREE_TEXT {
            assert!(is_allowed(name));
        }
    }

    #[test]
    fn names_are_snake_case_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in ALLOWED.iter().chain(FORBIDDEN) {
            assert!(seen.insert(*name), "{name} listed twice");
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()));
        }
    }
}
