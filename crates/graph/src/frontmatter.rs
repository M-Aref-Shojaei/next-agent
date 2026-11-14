// Format follows Logseq (AGPL-3.0, © Logseq contributors): YAML frontmatter
// read as page properties, deps/graph-parser/src/logseq/graph_parser/
// extract.cljc @ c67b8b5 (tag 0.10.9). Re-implemented in Rust.
//! Minimal YAML frontmatter reader: flat `key: value`, inline lists
//! `[a, b]` and `- item` lists. Values are returned as Logseq-style
//! comma-separated strings. The frontmatter text itself is never rewritten.

use crate::inline::{normalize_key, Property};

/// Byte offset just past the closing `---` line (0 when there is none).
#[must_use]
pub fn body_offset(source: &str) -> usize {
    let mut lines = source.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return 0;
    };
    if first.trim_end() != "---" {
        return 0;
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if line.trim_end() == "---" {
            return offset;
        }
    }
    0
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

/// Page properties from frontmatter, in file order.
#[must_use]
pub fn parse(source: &str) -> Vec<Property> {
    let end = body_offset(source);
    if end == 0 {
        return Vec::new();
    }
    let body = &source[..end];
    let mut out: Vec<Property> = Vec::new();
    let mut list_for: Option<usize> = None;
    for line in body.lines().skip(1) {
        let line = line.trim_end_matches('\r');
        if line.trim_end() == "---" {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let (Some(idx), Some(item)) = (list_for, trimmed.strip_prefix("- ")) {
            let prop = &mut out[idx];
            if !prop.value.is_empty() {
                prop.value.push_str(", ");
            }
            prop.value.push_str(&unquote(item));
            continue;
        }
        if line.starts_with([' ', '\t']) {
            continue; // nested YAML we don't model
        }
        let Some((key, raw)) = line.split_once(':') else {
            list_for = None;
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            list_for = None;
            continue;
        }
        let raw = raw.trim();
        let value = if let Some(inner) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inner
                .split(',')
                .map(unquote)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            unquote(raw)
        };
        out.push(Property {
            key: normalize_key(key),
            raw_key: key.to_string(),
            value,
        });
        list_for = raw.is_empty().then_some(out.len() - 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flat_inline_and_block_lists() {
        let src = "---\ntitle: \"My: Page\"\ntags: [a, \"b c\"]\nalias:\n  - x\n  - y\nnested:\n  k: v\n---\nbody";
        let props = parse(src);
        let pairs: Vec<_> = props
            .iter()
            .map(|p| (p.key.as_str(), p.value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("title", "My: Page"),
                ("tags", "a, b c"),
                ("alias", "x, y"),
                ("nested", "")
            ]
        );
        assert_eq!(&src[body_offset(src)..], "body");
    }

    #[test]
    fn no_frontmatter() {
        assert_eq!(parse("title:: x\n"), []);
        assert_eq!(body_offset("---\nunterminated"), 0);
    }
}
