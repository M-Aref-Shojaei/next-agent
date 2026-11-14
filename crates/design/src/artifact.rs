// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// apps/web/src/artifacts/parser.ts, markdown-context.ts and recover.ts
// @ 59e4f2a — modified: a whole-response (not streaming) Rust parser that
// returns every artifact, ignores `<artifact` inside code fences and inline
// code, completes an unclosed final artifact, and falls back to a fenced or
// bare HTML document.
//! `<artifact identifier="…" type="text/html" title="…">…</artifact>` blocks.

use serde::{Deserialize, Serialize};

/// One artifact the model returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub identifier: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    /// True when the closing tag was missing (the response was cut off).
    #[serde(default)]
    pub truncated: bool,
}

const OPEN: &str = "<artifact";
const CLOSE: &str = "</artifact>";

/// Byte ranges of fenced code blocks (```` ``` ```` / `~~~`) and inline code
/// spans, where `<artifact` is just text.
fn skip_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(usize, String)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker: String = trimmed
            .chars()
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        let is_fence = marker.len() >= 3
            && marker
                .chars()
                .all(|c| c == marker.chars().next().unwrap_or('`'));
        match &fence {
            Some((start, open))
                if is_fence
                    && marker.starts_with(open.as_str())
                    && trimmed.trim_end().len() == marker.len() =>
            {
                ranges.push((*start, offset + line.len()));
                fence = None;
            }
            None if is_fence => fence = Some((offset, marker)),
            None => {
                // Inline code spans on this line.
                let mut i = 0;
                let bytes = line.as_bytes();
                while i < bytes.len() {
                    if bytes[i] == b'`' {
                        if let Some(end) = line[i + 1..].find('`') {
                            ranges.push((offset + i, offset + i + 1 + end + 1));
                            i += end + 2;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
            Some(_) => {}
        }
        offset += line.len();
    }
    if let Some((start, _)) = fence {
        ranges.push((start, text.len()));
    }
    ranges
}

fn in_ranges(ranges: &[(usize, usize)], at: usize) -> bool {
    ranges.iter().any(|(s, e)| at >= *s && at < *e)
}

/// Parse `name="value"` / `name='value'` attributes.
#[must_use]
pub fn attributes(head: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = head.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
            let name = &head[start..i];
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'=' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let q = bytes[i];
                    let vstart = i + 1;
                    if let Some(len) = head[vstart..].bytes().position(|b| b == q) {
                        out.push((name.to_string(), head[vstart..vstart + len].to_string()));
                        i = vstart + len + 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Find the `>` that ends an opening tag, skipping quoted attribute values.
fn tag_end(text: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (i, ch) in text[from..].char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => return Some(from + i),
            _ => {}
        }
    }
    None
}

/// Every artifact in a model response, in order. When there is none, a
/// fenced ```` ```html ```` block or a bare `<!doctype html>` / `<html>`
/// document is recovered as one untitled artifact.
#[must_use]
pub fn parse(text: &str) -> Vec<Artifact> {
    let skip = skip_ranges(text);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = text[from..].find(OPEN) {
        let start = from + pos;
        let after = text.as_bytes().get(start + OPEN.len()).copied();
        let real = matches!(after, Some(b' ' | b'>' | b'\n' | b'\t' | b'\r'));
        if !real || in_ranges(&skip, start) {
            from = start + OPEN.len();
            continue;
        }
        let Some(head_end) = tag_end(text, start + OPEN.len()) else {
            break;
        };
        let attrs = attributes(&text[start + OPEN.len()..head_end]);
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let body_start = head_end + 1;
        let (content, truncated, next) = match text[body_start..].find(CLOSE) {
            Some(len) => (
                &text[body_start..body_start + len],
                false,
                body_start + len + CLOSE.len(),
            ),
            None => (&text[body_start..], true, text.len()),
        };
        out.push(Artifact {
            identifier: get("identifier").unwrap_or_else(|| format!("artifact-{}", out.len() + 1)),
            kind: get("type").unwrap_or_else(|| "text/html".into()),
            title: get("title").unwrap_or_default(),
            content: content.trim().to_string(),
            truncated,
        });
        from = next;
    }
    if out.is_empty() {
        if let Some(html) = recover_html(text) {
            out.push(Artifact {
                identifier: "artifact-1".into(),
                kind: "text/html".into(),
                title: String::new(),
                content: html,
                truncated: false,
            });
        }
    }
    out
}

/// A full HTML document from a ```` ```html ```` fence or bare in the text.
#[must_use]
pub fn recover_html(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if let Some(fence) = lower.find("```html") {
        let body = fence + 7;
        let body = text[body..].find('\n').map_or(body, |n| body + n + 1);
        let end = text[body..].find("```").map_or(text.len(), |e| body + e);
        let html = text[body..end].trim();
        if !html.is_empty() {
            return Some(html.to_string());
        }
    }
    let start = lower
        .find("<!doctype html")
        .or_else(|| lower.find("<html"))?;
    let end = lower.rfind("</html>").map_or(text.len(), |e| e + 7);
    (end > start).then(|| text[start..end].trim().to_string())
}

/// Text outside every artifact (the model's prose), trimmed.
#[must_use]
pub fn prose(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find(CLOSE) {
            rest = &rest[start + end + CLOSE.len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_several_artifacts() {
        let text = "Here are two.\n<artifact identifier=\"calm\" type=\"text/html\" title='Calm &amp; quiet'>\n<!doctype html><html></html>\n</artifact>\nand\n<artifact identifier=\"bold\" title=\"Bold > loud\"><html>b</html></artifact>";
        let a = parse(text);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].identifier, "calm");
        assert_eq!(a[0].title, "Calm &amp; quiet");
        assert_eq!(a[0].content, "<!doctype html><html></html>");
        assert_eq!(a[1].title, "Bold > loud");
        assert_eq!(a[1].kind, "text/html");
        assert!(!a[1].truncated);
        assert_eq!(prose(text), "Here are two.\n\nand");
    }

    #[test]
    fn ignores_artifacts_in_code() {
        let text = "Use `<artifact identifier=\"x\">` like this:\n```\n<artifact identifier=\"y\">no</artifact>\n```\n<artifactual>\n<artifact identifier=\"real\">yes</artifact>";
        let a = parse(text);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].identifier, "real");
        assert_eq!(a[0].content, "yes");
    }

    #[test]
    fn completes_an_unclosed_artifact() {
        let a = parse("<artifact identifier=\"cut\"><html><body>half");
        assert_eq!(a.len(), 1);
        assert!(a[0].truncated);
        assert_eq!(a[0].content, "<html><body>half");
    }

    #[test]
    fn recovers_fenced_or_bare_html() {
        let a = parse("Sure!\n```html\n<!doctype html><html>x</html>\n```\n");
        assert_eq!(a[0].content, "<!doctype html><html>x</html>");
        let b = parse("text <!DOCTYPE html><html>y</html> trailing");
        assert_eq!(b[0].content, "<!DOCTYPE html><html>y</html>");
        assert_eq!(parse("no html here"), vec![]);
    }

    #[test]
    fn missing_attributes_get_defaults() {
        let a = parse("<artifact>z</artifact><artifact >w</artifact>");
        assert_eq!(a[0].identifier, "artifact-1");
        assert_eq!(a[1].identifier, "artifact-2");
    }

    #[test]
    fn attribute_reader() {
        assert_eq!(
            attributes(" a=\"1\" b = '2' c d=\"x y\""),
            vec![
                ("a".into(), "1".into()),
                ("b".into(), "2".into()),
                ("d".into(), "x y".into())
            ]
        );
    }
}
