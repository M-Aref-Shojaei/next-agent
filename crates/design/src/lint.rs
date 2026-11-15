// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// apps/daemon/src/lint-artifact.ts @ 59e4f2a — modified: a subset of its
// rules (purple-gradient, ai-default-indigo, emoji-icon, filler-copy,
// all-caps-no-tracking, external-image, raw-hex) re-implemented in Rust, plus
// our own sandbox rules (network, script) and an off-token colour check.
//! Heuristic checks on a generated direction, shown on its card and fed
//! back to the model during self-critique.

use crate::css;
use serde::{Deserialize, Serialize};

/// How many off-palette colours are named in the `off-palette` message.
const MAX_OFF_PALETTE_LISTED: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    P0,
    P1,
    P2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
}

fn finding(rule: &str, severity: Severity, message: impl Into<String>) -> Finding {
    Finding {
        rule: rule.into(),
        severity,
        message: message.into(),
    }
}

/// Everything inside `:root { … }` blocks (where raw hex is fine).
fn root_blocks(html: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find(":root") {
        let start = from + i;
        let Some(open) = html[start..].find('{') else {
            break;
        };
        let open = start + open;
        let close = html[open..].find('}').map_or(html.len(), |c| open + c);
        out.push((start, close));
        from = close.max(start + ":root".len());
    }
    out
}

/// Lint one direction. `token_colors` are the design system's colour values
/// (lower-case); colours outside them are reported once, in summary.
#[must_use]
pub fn lint(html: &str, token_colors: &[String]) -> Vec<Finding> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();

    out.extend(network_finding(&lower));
    if lower.contains("<script") {
        out.push(finding(
            "script",
            Severity::P1,
            "Contains a script; previews don't run scripts, so interactions won't show.",
        ));
    }
    let gradient_purple = lower.contains("gradient(")
        && [
            "purple", "violet", "#8b5cf6", "#a855f7", "#7c3aed", "#9333ea", "#c084fc",
        ]
        .iter()
        .any(|c| lower.contains(c));
    if gradient_purple {
        out.push(finding(
            "purple-gradient",
            Severity::P0,
            "Purple gradient: the default AI look. Use the project's accent instead.",
        ));
    }
    if ["#6366f1", "#4f46e5", "#4338ca", "indigo-500", "indigo-600"]
        .iter()
        .any(|c| lower.contains(c))
    {
        out.push(finding(
            "ai-default-indigo",
            Severity::P0,
            "Default indigo; not from the design system.",
        ));
    }
    if html.chars().any(is_emoji) {
        out.push(finding(
            "emoji-icon",
            Severity::P0,
            "Emoji used as icons; use simple inline SVG or text.",
        ));
    }
    if lower.contains("lorem ipsum") || lower.contains("dolor sit amet") {
        out.push(finding(
            "filler-copy",
            Severity::P0,
            "Lorem ipsum filler; write copy specific to the brief.",
        ));
    }
    out.extend(css_findings(html));
    out.extend(raw_hex_findings(html, token_colors));
    if !lower.contains("name=\"viewport\"") && !lower.contains("name=viewport") {
        out.push(finding(
            "missing-viewport",
            Severity::P2,
            "No viewport meta (the preview adds one).",
        ));
    }
    out.sort_by_key(|f| f.severity);
    out
}

/// Loads something from the network, which previews cannot fetch.
fn network_finding(lower: &str) -> Vec<Finding> {
    let external = [
        "src=\"http",
        "src='http",
        "href=\"http",
        "href='http",
        "url(http",
        "url(\"http",
        "url('http",
        "src=\"//",
        "href=\"//",
        "@import",
    ]
    .iter()
    .any(|p| lower.contains(p));
    if external {
        vec![finding("network", Severity::P0, "Loads something from the network; previews have no network, so it will be missing. Inline it or use a CSS shape.")]
    } else {
        Vec::new()
    }
}

/// Uppercase text in inline CSS with no letter-spacing alongside it.
fn css_findings(html: &str) -> Vec<Finding> {
    let decls = css::declarations(&css_of(html));
    for decl in &decls {
        if decl.property == "text-transform" && decl.value == "uppercase" {
            let tracked = decls
                .iter()
                .any(|d| d.context == decl.context && d.property == "letter-spacing");
            if !tracked {
                return vec![finding(
                    "all-caps-no-tracking",
                    Severity::P1,
                    "Uppercase text without letter-spacing (add about 0.06em).",
                )];
            }
        }
    }
    Vec::new()
}

/// Raw hex outside `:root`, and colours that aren't design tokens.
fn raw_hex_findings(html: &str, token_colors: &[String]) -> Vec<Finding> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let roots = root_blocks(&lower);
    let mut raw = 0usize;
    let mut off: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(p) = lower[i..].find('#') {
        let at = i + p;
        i = at + 1;
        let window: String = lower[at..].chars().take(10).collect();
        let found = css::colors_in(&window);
        let Some(color) = found.first() else { continue };
        if !lower[at..].starts_with(color.as_str()) || roots.iter().any(|(s, e)| at > *s && at < *e)
        {
            continue;
        }
        raw += 1;
        if !token_colors.is_empty() && !token_colors.contains(color) && !off.contains(color) {
            off.push(color.clone());
        }
    }
    if raw > 0 {
        out.push(finding(
            "raw-hex",
            Severity::P1,
            format!("{raw} raw hex colour(s) outside :root; use var(--token)."),
        ));
    }
    if !off.is_empty() {
        off.truncate(MAX_OFF_PALETTE_LISTED);
        out.push(finding(
            "off-palette",
            Severity::P1,
            format!("Colours not in tokens.css: {}.", off.join(", ")),
        ));
    }
    out
}

fn css_of(html: &str) -> String {
    css::style_blocks(html).join("\n")
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0x1F000..=0x1F2FF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(html: &str, tokens: &[&str]) -> Vec<String> {
        let tokens: Vec<String> = tokens.iter().map(|s| (*s).to_string()).collect();
        lint(html, &tokens).into_iter().map(|f| f.rule).collect()
    }

    #[test]
    fn clean_direction_passes() {
        let html = "<html><head><meta name=\"viewport\" content=\"width=device-width\"><style>:root{--accent:#3b5bdb} h1{color:var(--accent)} .k{text-transform:uppercase;letter-spacing:.06em}</style></head><body><h1>Pipelines</h1></body></html>";
        assert!(
            rules(html, &["#3b5bdb"]).is_empty(),
            "{:?}",
            rules(html, &["#3b5bdb"])
        );
    }

    #[test]
    fn flags_open_design_rules() {
        let html = "<style>.hero{background:linear-gradient(#8b5cf6,#6366f1)} .t{text-transform:uppercase}</style><p>🚀 Lorem ipsum</p><img src=\"https://picsum.photos/200\"><script>x()</script>";
        let r = rules(html, &["#3b5bdb"]);
        for rule in [
            "network",
            "script",
            "purple-gradient",
            "ai-default-indigo",
            "emoji-icon",
            "filler-copy",
            "all-caps-no-tracking",
            "raw-hex",
            "off-palette",
            "missing-viewport",
        ] {
            assert!(r.contains(&rule.to_string()), "{rule} in {r:?}");
        }
        // P0 first.
        assert_eq!(lint(html, &[])[0].severity, Severity::P0);
    }

    #[test]
    fn hash_before_multibyte_char_does_not_panic() {
        let _ = lint("<p>#— hi</p>", &[]);
    }
}
