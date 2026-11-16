//! Redaction of every string the log writes (ADR-0013, rule 3).
//!
//! In order: URLs keep only their scheme and host; secret shapes become
//! `[secret]`; emails and phone numbers become `[redacted]`; the home folder
//! becomes `~`; absolute paths shrink to their depth and extension; long hex
//! or base64 runs become `[secret]`; the result is cut to 300 characters.

use regex::{Captures, Regex};
use std::sync::LazyLock;

/// Longest string the log keeps, in characters.
pub const MAX_CHARS: usize = 300;
const SECRET: &str = "[secret]";
const REDACTED: &str = "[redacted]";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("redaction pattern is valid")
}

/// `scheme://[userinfo@]host[:port]` and whatever follows (path, query).
static URL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b([a-z][a-z0-9+.-]*://)(?:[^\s/@]*@)?([^\s/?#'\x22<>]+)([^\s'\x22<>]*)")
});

/// Credential shapes. Each whole match becomes `[secret]`.
static SECRETS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // Anthropic, OpenAI and compatible keys (sk-ant-…, sk-proj-…, sk-…).
        r"\bsk-[A-Za-z0-9_-]{8,}",
        // Google API keys.
        r"\bAIza[0-9A-Za-z_-]{20,}",
        // GitHub tokens.
        r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{16,}",
        r"\bgithub_pat_[A-Za-z0-9_]{16,}",
        // Slack tokens.
        r"\bxox[abposr]-[A-Za-z0-9-]{8,}",
        // Telegram bot tokens: <bot id>:<35 characters>.
        r"\b\d{5,}:[A-Za-z0-9_-]{30,}",
        // HTTP credentials.
        r"(?i)\b(?:bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}",
        // JWTs.
        r"\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*",
    ]
    .into_iter()
    .map(re)
    .collect()
});

/// `api_key=…`, `token: …`, `password=…`: keep the name, hide the value.
static ASSIGNED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passwd|authorization|cookie)(\s*[:=]\s*)[^\s,;&]+",
    )
});

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"));

static PHONES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"\+\d[\d\s().-]{6,}\d",
        r"\(\d{3}\)\s*\d{3}[-.\s]\d{4}\b",
        r"\b\d{3}[-.]\d{3}[-.]\d{4}\b",
    ]
    .into_iter()
    .map(re)
    .collect()
});

/// Any user's home folder, on macOS, Linux and Windows.
static HOMES: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?:/Users/|/home/|(?i:[A-Z]:\\Users\\))[^\s/\\'\x22]+"));

/// An absolute path (or one under `~`), after a boundary. Group 1 is the
/// boundary, group 2 the path.
static PATH: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(^|[\s"'(=,\[])((?:~|[A-Za-z]:)?[/\\][^\s"'<>(),\[\]]*)"#));

static HEX: LazyLock<Regex> = LazyLock::new(|| re(r"\b[0-9a-fA-F]{32,}\b"));
static BASE64: LazyLock<Regex> = LazyLock::new(|| re(r"[A-Za-z0-9+/_=-]{40,}"));

/// The current user's home folder, when it is long enough to be worth hiding.
static HOME: LazyLock<Option<String>> = LazyLock::new(|| {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|v| {
            v.to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .to_string()
        })
        .find(|v| v.len() > 1)
});

/// Redact one string for the log. Safe on any input; never panics.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = URL
        .replace_all(text, |c: &Captures| {
            let rest = if c[3].is_empty() { "" } else { "/…" };
            format!("{}{}{rest}", &c[1], &c[2])
        })
        .into_owned();
    for pattern in SECRETS.iter() {
        out = pattern.replace_all(&out, SECRET).into_owned();
    }
    out = ASSIGNED_SECRET
        .replace_all(&out, |c: &Captures| format!("{}{}{SECRET}", &c[1], &c[2]))
        .into_owned();
    out = EMAIL.replace_all(&out, REDACTED).into_owned();
    for pattern in PHONES.iter() {
        out = pattern.replace_all(&out, REDACTED).into_owned();
    }
    if let Some(home) = HOME.as_deref() {
        out = out.replace(home, "~");
    }
    out = HOMES.replace_all(&out, "~").into_owned();
    out = PATH
        .replace_all(&out, |c: &Captures| {
            format!("{}{}", &c[1], shorten_path(&c[2]))
        })
        .into_owned();
    out = HEX.replace_all(&out, SECRET).into_owned();
    out = BASE64
        .replace_all(&out, |c: &Captures| {
            let run = &c[0];
            let mixed = run.bytes().any(|b| b.is_ascii_digit())
                && run.bytes().any(|b| b.is_ascii_uppercase())
                && run.bytes().any(|b| b.is_ascii_lowercase());
            if mixed {
                SECRET.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned();
    truncate(&out)
}

/// `~/code/app/src/main.rs` → `~/…/*.rs (depth 4)`. A path of one segment
/// (`/tmp`, `~/x`) is kept.
fn shorten_path(path: &str) -> String {
    let (root, rest) = if let Some(rest) = path.strip_prefix('~') {
        ("~", rest)
    } else if path.as_bytes().get(1) == Some(&b':') {
        (&path[..2], &path[2..])
    } else {
        ("", path)
    };
    let segments: Vec<&str> = rest.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    // Already shortened (`~/…/*.rs`): redaction must be idempotent.
    if segments.len() < 2 || segments.contains(&"…") {
        return path.to_string();
    }
    let depth = segments.len();
    let last = segments[depth - 1];
    match last.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 8
                && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            format!("{root}/…/*.{ext} (depth {depth})")
        }
        _ => format!("{root}/… (depth {depth})"),
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_shapes_are_hidden() {
        for secret in [
            "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789",
            "sk-proj-abcdefghijklmnop1234",
            "AIzaSyA1234567890abcdefghijklmnopq",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            "github_pat_11ABCDEFG0123456789_abcdefghijk",
            "xoxb-1234567890-abcdefghij",
            "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw1",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "0123456789abcdef0123456789abcdef01",
            "QWxhZGRpbjpvcGVuIHNlc2FtZQ9xY2hhbGxlbmdlcmVzcG9uc2U",
        ] {
            let out = redact(&format!("failed with {secret} today"));
            assert!(!out.contains(secret), "{secret} leaked: {out}");
            assert!(out.contains("[secret]"), "{out}");
        }
    }

    #[test]
    fn bearer_and_assigned_secrets_keep_their_name_only() {
        let out = redact("Authorization: Bearer abc.def-ghi_jkl123");
        assert!(!out.contains("abc.def"), "{out}");
        let out = redact("request had api_key=hunter2 and password: swordfish");
        assert_eq!(out, "request had api_key=[secret] and password: [secret]");
    }

    #[test]
    fn urls_keep_scheme_and_host_only() {
        assert_eq!(
            redact("POST https://api.telegram.org/bot123456:AAAA/sendMessage?chat_id=9 failed"),
            "POST https://api.telegram.org/… failed"
        );
        assert_eq!(
            redact("to https://user:pw@example.com"),
            "to https://example.com"
        );
        assert_eq!(
            redact("http://127.0.0.1:20128/v1"),
            "http://127.0.0.1:20128/…"
        );
    }

    #[test]
    fn emails_and_phones_are_redacted() {
        assert_eq!(redact("from jane.doe+x@example.co.uk"), "from [redacted]");
        assert_eq!(redact("call +44 20 7946 0958 now"), "call [redacted] now");
        assert_eq!(redact("call (555) 123-4567"), "call [redacted]");
        assert_eq!(redact("call 555-123-4567"), "call [redacted]");
        // Dates, versions and counts are not phone numbers.
        assert_eq!(
            redact("2026-11-15 v1.2.3 status 529"),
            "2026-11-15 v1.2.3 status 529"
        );
    }

    #[test]
    fn home_folders_and_paths_shrink() {
        assert_eq!(
            redact("open /Users/alice/code/app/src/main.rs"),
            "open ~/…/*.rs (depth 4)"
        );
        assert_eq!(
            redact("read \"/Users/alice/a/b\" failed"),
            "read \"~/… (depth 2)\" failed"
        );
        assert_eq!(
            redact("in /home/bob/vault/pages/Plan.md"),
            "in ~/…/*.md (depth 3)"
        );
        assert_eq!(
            redact("at /var/lib/x/db.sqlite"),
            "at /…/*.sqlite (depth 4)"
        );
        assert_eq!(
            redact(r"C:\Users\carol\Documents\secret plan.docx"),
            "~/… (depth 2) plan.docx"
        );
        assert_eq!(redact("in /tmp"), "in /tmp");
    }

    #[test]
    fn current_home_becomes_tilde() {
        if let Some(home) = HOME.as_deref() {
            let out = redact(&format!("{home}/projects/x/y.rs"));
            assert!(!out.contains(home), "{out}");
            assert!(out.starts_with('~'), "{out}");
        }
    }

    #[test]
    fn ids_and_plain_words_survive() {
        for text in [
            "t_01J9ZK3Q4X5Y6Z7A8B9C0D1E2F",
            "claude-sonnet-4-5",
            "connection refused",
            "status 529 overloaded_error",
            "3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708",
        ] {
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn redaction_is_idempotent() {
        for text in [
            "open /Users/alice/code/app/src/main.rs",
            "GET https://example.com/a/b?q=1 with sk-ant-abcdefghijklmnop",
            "mail bob@example.com from /var/lib/x",
        ] {
            let once = redact(text);
            assert_eq!(redact(&once), once);
        }
    }

    #[test]
    fn long_strings_are_cut_to_300_characters() {
        let out = redact(&"é ".repeat(400));
        assert_eq!(out.chars().count(), MAX_CHARS);
        assert!(out.ends_with('…'));
    }
}
