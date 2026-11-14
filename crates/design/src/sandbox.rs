// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// apps/web/src/components/FileViewer.tsx (HTML_PREVIEW_SANDBOX) and
// apps/daemon/src/live-artifacts/http-helpers.ts (preview CSP) @ 59e4f2a —
// modified: directions render with an empty `sandbox` (no scripts, forms,
// popups or same-origin) and the no-network CSP is injected into the srcdoc
// itself, ahead of any author markup, together with the project's tokens.
//! Sandboxed previews for generated directions, and their device frames.

use serde::{Deserialize, Serialize};

/// `sandbox` for direction previews: everything off. Opaque origin, no
/// script, no forms, no popups, no top navigation.
pub const DIRECTION_SANDBOX: &str = "";

/// `sandbox` for the live app preview (served by the loopback proxy on its
/// own origin, so `allow-same-origin` is the *proxy's* origin, never the
/// studio's). No popups, no top navigation, no downloads.
pub const APP_SANDBOX: &str = "allow-scripts allow-same-origin allow-forms";

/// CSP injected first into every direction document: no network at all.
pub const DIRECTION_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; base-uri 'none'; form-action 'none'";

/// A device frame for previews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameKind {
    Web,
    Mobile,
    Desktop,
}

impl FrameKind {
    /// Viewport size in CSS pixels.
    #[must_use]
    pub fn size(self) -> (u32, u32) {
        match self {
            Self::Web => (1280, 800),
            Self::Mobile => (390, 844),
            Self::Desktop => (1440, 900),
        }
    }

    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Web => "a responsive web page in a 1280×800 browser viewport",
            Self::Mobile => "a mobile app screen at 390×844 (iPhone-sized), touch targets at least 44px",
            Self::Desktop => "a desktop application window at 1440×900 with app chrome (sidebar, toolbar), not a marketing page",
        }
    }

    /// Parse from the UI's name.
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name {
            "mobile" => Self::Mobile,
            "desktop" => Self::Desktop,
            _ => Self::Web,
        }
    }
}

/// Remove elements that could navigate or reach out even inside the
/// sandbox: `<meta http-equiv="refresh">`, `<base>`, `<link rel=prefetch|
/// preconnect|dns-prefetch|preload>`. (Scripts stay in the text but never
/// run: the frame has no `allow-scripts`.)
fn strip_reaching_tags(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while let Some(pos) = lower[i..].find('<') {
        let start = i + pos;
        out.push_str(&html[i..start]);
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |e| start + e + 1);
        let tag = &lower[start..end];
        let name: String = tag[1..]
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        let drop = match name.as_str() {
            "base" => true,
            "meta" => tag.contains("http-equiv") && tag.contains("refresh"),
            "link" => [
                "prefetch",
                "preconnect",
                "dns-prefetch",
                "preload",
                "prerender",
                "modulepreload",
            ]
            .iter()
            .any(|r| tag.contains(r)),
            _ => false,
        };
        if !drop {
            out.push_str(&html[start..end]);
        }
        i = end;
    }
    out.push_str(&html[i..]);
    out
}

/// Wrap a direction's HTML for `srcdoc`: the CSP `<meta>` and the project's
/// `tokens.css` go first in `<head>` (a CSP meta only governs what follows
/// it, and several policies intersect, so the author can only tighten it).
/// A fragment is wrapped in a full document.
#[must_use]
pub fn sandbox_document(html: &str, tokens_css: &str) -> String {
    let html = strip_reaching_tags(html);
    let tokens = tokens_css.replace("</style", "<\\/style");
    let preamble = format!(
        "<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{DIRECTION_CSP}\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><style data-next-agent-tokens>{tokens}</style>"
    );
    let lower = html.to_ascii_lowercase();
    let head_open = lower
        .find("<head")
        .and_then(|i| lower[i..].find('>').map(|e| i + e + 1));
    if let Some(at) = head_open {
        return format!("{}{preamble}{}", &html[..at], &html[at..]);
    }
    if let Some(at) = lower
        .find("<html")
        .and_then(|i| lower[i..].find('>').map(|e| i + e + 1))
    {
        return format!("{}<head>{preamble}</head>{}", &html[..at], &html[at..]);
    }
    format!("<!doctype html><html><head>{preamble}</head><body>{html}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_attributes_are_locked_down() {
        assert_eq!(DIRECTION_SANDBOX, "");
        for forbidden in [
            "allow-top-navigation",
            "allow-popups",
            "allow-downloads",
            "allow-modals",
        ] {
            assert!(!APP_SANDBOX.contains(forbidden));
        }
        assert!(DIRECTION_CSP.starts_with("default-src 'none'"));
        assert!(!DIRECTION_CSP.contains("http"));
        assert!(!DIRECTION_CSP.contains("script-src"));
    }

    #[test]
    fn csp_goes_first_in_head() {
        let doc = sandbox_document("<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\" content=\"default-src *\"><title>t</title></head><body>x</body></html>", ":root{--a:#fff}");
        let ours = doc.find(DIRECTION_CSP).expect("csp");
        let theirs = doc.find("default-src *").expect("author");
        assert!(ours < theirs);
        assert!(doc.contains("<style data-next-agent-tokens>:root{--a:#fff}</style>"));
    }

    #[test]
    fn fragments_are_wrapped_and_tokens_cannot_break_out() {
        let doc = sandbox_document("<p>hi</p>", "</style><script>alert(1)</script>");
        assert!(doc.starts_with("<!doctype html><html><head><meta charset"));
        assert!(doc.ends_with("<body><p>hi</p></body></html>"));
        assert!(!doc.contains("</style><script>"));
        let no_head = sandbox_document("<html><body>x</body></html>", "");
        assert!(no_head.starts_with("<html><head><meta charset"));
    }

    #[test]
    fn strips_navigation_and_prefetch_tags() {
        let doc = sandbox_document(
            "<head><meta http-equiv=\"refresh\" content=\"0;url=http://127.0.0.1:8080\"><base href=\"http://x/\"><link rel=\"preconnect\" href=\"https://x\"><link rel=\"stylesheet\" href=\"a.css\"></head>",
            "",
        );
        assert!(!doc.contains("refresh") && !doc.contains("<base") && !doc.contains("preconnect"));
        assert!(
            doc.contains("rel=\"stylesheet\""),
            "kept (blocked by CSP, flagged by lint)"
        );
    }

    #[test]
    fn frames() {
        assert_eq!(FrameKind::parse("mobile").size(), (390, 844));
        assert_eq!(FrameKind::parse("desktop"), FrameKind::Desktop);
        assert_eq!(FrameKind::parse("anything"), FrameKind::Web);
    }
}
