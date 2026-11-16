//! Redaction of every string the log writes (ADR-0013, rule 3).
//!
//! In order: URLs keep only their scheme and host; secret shapes become
//! `[secret]`; emails and phone numbers become `[redacted]`; the home folder
//! becomes `~`; absolute paths shrink to their depth and extension; long hex
//! or base64 runs become `[secret]`; the result is cut to 300 characters.

use regex::{Captures, Regex};
use std::borrow::Cow;
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

/// A relative path (`pages/My plan.md`, `a/b/c`): a word, then at least one
/// separator. Same groups as [`PATH`].
static RELATIVE_PATH: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(^|[\s"'(=,\[])([\w.…-]+(?:[/\\][^\s"'<>(),\[\]/\\]+)+[/\\]?)"#));

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
    // Work on a bounded prefix: the result is cut to 300 characters anyway,
    // and the margin keeps a secret near the cut from being split before
    // its pattern can see it.
    let mut out: String = text.chars().take(INPUT_CHARS).collect();
    replace(&mut out, &URL, |c| {
        let rest = if c[3].is_empty() { "" } else { "/…" };
        format!("{}{}{rest}", &c[1], &c[2])
    });
    for pattern in SECRETS.iter() {
        replace(&mut out, pattern, |_| SECRET.into());
    }
    replace(&mut out, &ASSIGNED_SECRET, |c| {
        format!("{}{}{SECRET}", &c[1], &c[2])
    });
    replace(&mut out, &EMAIL, |_| REDACTED.into());
    for pattern in PHONES.iter() {
        replace(&mut out, pattern, |_| REDACTED.into());
    }
    if let Some(home) = HOME.as_deref() {
        if out.contains(home) {
            out = out.replace(home, "~");
        }
    }
    replace(&mut out, &HOMES, |_| "~".into());
    replace(&mut out, &PATH, |c| {
        format!("{}{}", &c[1], shorten_path(&c[2], false))
    });
    replace(&mut out, &RELATIVE_PATH, |c| {
        format!("{}{}", &c[1], shorten_path(&c[2], true))
    });
    replace(&mut out, &HEX, |_| SECRET.into());
    replace(&mut out, &BASE64, |c| {
        let run = &c[0];
        let mixed = run.bytes().any(|b| b.is_ascii_digit())
            && run.bytes().any(|b| b.is_ascii_uppercase())
            && run.bytes().any(|b| b.is_ascii_lowercase());
        if mixed {
            SECRET.into()
        } else {
            run.into()
        }
    });
    truncate(&out)
}

/// Characters of the input that redaction looks at.
const INPUT_CHARS: usize = 4096;

/// One pass; allocates only when the pattern matched.
fn replace(out: &mut String, pattern: &Regex, with: impl Fn(&Captures) -> String) {
    let replaced = pattern.replace_all(out, |c: &Captures| with(c));
    if let Cow::Owned(changed) = replaced {
        *out = changed;
    }
}

/// `~/code/app/src/main.rs` → `~/…/*.rs (depth 4)`; relative
/// `pages/plan.md` → `…/*.md (depth 2)`. Kept: a one-segment absolute path
/// (`/tmp`), a two-segment relative one without a file extension
/// (`openai/gpt-4o`, `text/html`), and an already shortened path.
fn shorten_path(path: &str, relative: bool) -> String {
    let (root, rest) = if relative {
        ("…", path)
    } else if let Some(rest) = path.strip_prefix('~') {
        ("~/…", rest)
    } else if path.as_bytes().get(1) == Some(&b':') {
        (&path[..2], &path[2..])
    } else {
        ("/…", path)
    };
    let segments: Vec<&str> = rest.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    // Exactly the shape this function writes (`~/…/*.rs`, `~/…`): redaction
    // must be idempotent. Any other path with a `…` segment is shortened.
    let shortened = segments.first() == Some(&"…")
        && (segments.len() == 1 || (segments.len() == 2 && segments[1].starts_with("*.")));
    let ext = segments.last().and_then(|last| extension(last));
    let keep = if relative {
        segments.len() < 3 && ext.is_none()
    } else {
        segments.len() < 2
    };
    if keep || shortened {
        return path.to_string();
    }
    let root = if root.ends_with(':') {
        format!("{root}/…")
    } else {
        root.to_string()
    };
    let depth = segments.len();
    match ext {
        Some(ext) => format!("{root}/*.{ext} (depth {depth})"),
        None => format!("{root} (depth {depth})"),
    }
}

/// A file extension: up to 8 ASCII letters and digits, at least one letter,
/// after a non-empty stem (`5` in `claude-3.5` is not one).
fn extension(segment: &str) -> Option<&str> {
    let (stem, ext) = segment.rsplit_once('.')?;
    let ok = !stem.is_empty()
        && (1..=8).contains(&ext.len())
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        && ext.bytes().any(|b| b.is_ascii_alphabetic());
    ok.then_some(ext)
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
    fn relative_paths_shrink_but_model_names_and_mime_types_stay() {
        assert_eq!(
            redact("read pages/My-diary.md failed"),
            "read …/*.md (depth 2) failed"
        );
        assert_eq!(redact("in notes/2026/secret"), "in … (depth 3)");
        for kept in ["openai/gpt-4o", "anthropic/claude-3.5", "text/html", "5/10"] {
            assert_eq!(redact(kept), kept);
        }
    }

    #[test]
    fn a_planted_ellipsis_segment_does_not_skip_shortening() {
        let out = redact("/Users/x/…/secret-folder/plan.md");
        assert_eq!(out, "~/…/*.md (depth 3)");
        let out = redact("…/secret-folder/plan.md");
        assert!(!out.contains("secret-folder"), "{out}");
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
