// Format follows Logseq (AGPL-3.0, © Logseq contributors):
// deps/graph-parser/src/logseq/graph_parser/{block.cljs,property.cljs,
// util/page_ref.cljs,util/block_ref.cljs} @ c67b8b5 (tag 0.10.9).
// Re-implemented in Rust.
//! Facts derived from a block's text: properties, task marker, priority,
//! `SCHEDULED`/`DEADLINE`, page refs, tags, block refs and embeds.
//!
//! Derivation is read-only: the text itself is never rewritten here, so
//! unknown syntax (macros, drawers, org leftovers) survives verbatim.

use serde::{Deserialize, Serialize};

/// Task markers Logseq recognises at the start of a block.
pub const MARKERS: &[&str] = &[
    "TODO",
    "DOING",
    "DONE",
    "NOW",
    "LATER",
    "WAITING",
    "WAIT",
    "CANCELED",
    "CANCELLED",
    "STARTED",
    "IN-PROGRESS",
];

/// Properties Logseq hides in the UI (`hidden-built-in-properties`).
pub const HIDDEN_PROPERTIES: &[&str] = &[
    "id",
    "custom-id",
    "background-color",
    "heading",
    "collapsed",
    "created-at",
    "updated-at",
    "last-modified-at",
    "query-table",
    "query-properties",
    "query-sort-by",
    "query-sort-desc",
    "ls-type",
    "hl-type",
    "hl-page",
    "hl-stamp",
    "hl-color",
    "logseq.macro-name",
    "logseq.macro-arguments",
    "logseq.order-list-type",
    "logseq.tldraw.page",
    "logseq.tldraw.shape",
];

/// Properties whose comma-separated values are page references.
const LINK_LIST_PROPERTIES: &[&str] = &["alias", "aliases", "tags"];

/// One `key:: value` property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    /// Normalised key: lowercase, spaces and `_` replaced by `-`.
    pub key: String,
    /// The key as written.
    pub raw_key: String,
    pub value: String,
}

/// How a page or block is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    /// `[[Page]]`
    Link,
    /// `#tag` or `#[[multi word]]`
    Tag,
    /// A value of `tags::`/`alias::` or a link inside a property value.
    Property,
    /// `{{embed [[Page]]}}` / `{{embed ((uuid))}}`
    Embed,
    /// `((uuid))`
    BlockRef,
}

/// A reference found in a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ref {
    Page { name: String, kind: RefKind },
    Block { uuid: String, kind: RefKind },
}

/// Everything derived from a block's content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub marker: Option<String>,
    /// `A`, `B` or `C` from `[#A]`.
    pub priority: Option<String>,
    /// Markdown heading level of the first line (`- ## x` → 2).
    pub heading: Option<u8>,
    /// First line without marker, priority and heading hashes.
    pub title: String,
    pub properties: Vec<Property>,
    /// Inside the angle brackets of `SCHEDULED: <…>`.
    pub scheduled: Option<String>,
    pub deadline: Option<String>,
    pub refs: Vec<Ref>,
    /// Searchable text: content without hidden property lines and drawers.
    pub text: String,
}

impl BlockInfo {
    #[must_use]
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.value.as_str())
    }

    /// The block's UUID from `id::` (or `custom-id::`/`custom_id::`).
    #[must_use]
    pub fn uuid(&self) -> Option<String> {
        self.property("id")
            .or_else(|| self.property("custom-id"))
            .map(str::trim)
            .filter(|v| is_uuid(v))
            .map(str::to_ascii_lowercase)
    }

    #[must_use]
    pub fn collapsed(&self) -> bool {
        self.property("collapsed") == Some("true")
    }
}

/// Logseq's property key normalisation.
#[must_use]
pub fn normalize_key(key: &str) -> String {
    key.to_lowercase().replace([' ', '_'], "-")
}

/// Parse a `key:: value` line.
#[must_use]
pub fn property_line(line: &str) -> Option<Property> {
    let at = line.find("::")?;
    let raw_key = line[..at].trim_start();
    let after = &line[at + 2..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let valid = !raw_key.is_empty()
        && !raw_key.starts_with('#')
        && !raw_key.chars().any(|c| {
            c.is_whitespace() || matches!(c, ':' | '"' | '(' | ')' | '{' | '}' | '[' | ']' | '^')
        });
    if !valid {
        return None;
    }
    Some(Property {
        key: normalize_key(raw_key),
        raw_key: raw_key.to_string(),
        value: after.trim().to_string(),
    })
}

/// `8-4-4-4-12` hex UUID.
#[must_use]
pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn is_drawer_start(t: &str) -> bool {
    t.len() > 2
        && t.starts_with(':')
        && t.ends_with(':')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && t != ":END:"
}

fn angle_value(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start().strip_prefix('<')?;
    let end = rest.find('>')?;
    Some(rest[..end].to_string())
}

/// Derive [`BlockInfo`] from block content.
#[must_use]
pub fn analyze(content: &str) -> BlockInfo {
    let mut info = BlockInfo::default();
    let lines: Vec<&str> = content.split('\n').collect();
    let first = lines.first().copied().unwrap_or("");

    let first_is_property = property_line(first).is_some();
    if !first_is_property {
        let (marker, priority, heading, title) = split_title(first);
        info.marker = marker;
        info.priority = priority;
        info.heading = heading;
        info.title = title;
    }

    let mut text_lines: Vec<&str> = Vec::new();
    let mut in_drawer = false;
    let mut in_fence = false;
    let mut zone_open = true;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if in_drawer {
            if t.eq_ignore_ascii_case(":END:") {
                in_drawer = false;
            }
            continue;
        }
        let fence_line = t.starts_with("```") || t.starts_with("~~~");
        if in_fence || fence_line {
            if in_fence == fence_line {
                in_fence = false;
            } else if fence_line {
                in_fence = true;
            }
            if i > 0 {
                zone_open = false;
            }
            text_lines.push(line);
            continue;
        }
        if i > 0 && is_drawer_start(t) {
            in_drawer = true;
            continue;
        }
        if t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:") {
            if let Some(v) = angle_value(t, "SCHEDULED:") {
                info.scheduled = Some(v);
            }
            if let Some(v) = angle_value(t, "DEADLINE:") {
                info.deadline = Some(v);
            }
            continue;
        }
        if zone_open && (i > 0 || first_is_property) {
            if let Some(p) = property_line(line) {
                let hidden = HIDDEN_PROPERTIES.contains(&p.key.as_str());
                if !hidden {
                    text_lines.push(line);
                }
                property_refs(&p, &mut info.refs);
                info.properties.push(p);
                continue;
            }
        }
        if i > 0 {
            zone_open = false;
        }
        if i == 0 {
            if first_is_property {
                continue;
            }
            scan_refs(first, &mut info.refs);
            text_lines.push(line);
        } else {
            scan_refs(line, &mut info.refs);
            text_lines.push(line);
        }
    }
    info.text = text_lines.join("\n").trim().to_string();
    info
}

type TitleParts = (Option<String>, Option<String>, Option<u8>, String);

fn split_title(first: &str) -> TitleParts {
    let mut rest = first;
    let mut heading = None;
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
        heading = u8::try_from(hashes).ok();
        rest = &rest[hashes + 1..];
    }
    let mut marker = None;
    for m in MARKERS {
        if let Some(after) = rest.strip_prefix(m) {
            if after.is_empty() || after.starts_with(' ') {
                marker = Some((*m).to_string());
                rest = after.trim_start();
                break;
            }
        }
    }
    let mut priority = None;
    if let Some(at) = rest.find("[#") {
        let tail = &rest[at + 2..];
        let mut chars = tail.chars();
        if let (Some(p @ ('A' | 'B' | 'C')), Some(']')) = (chars.next(), chars.next()) {
            priority = Some(p.to_string());
            let before = rest[..at].trim_end();
            let after = tail[2..].trim_start();
            let joined = match (before.is_empty(), after.is_empty()) {
                (true, _) => after.to_string(),
                (_, true) => before.to_string(),
                _ => format!("{before} {after}"),
            };
            return (marker, priority, heading, joined);
        }
    }
    (marker, priority, heading, rest.to_string())
}

fn property_refs(p: &Property, out: &mut Vec<Ref>) {
    if HIDDEN_PROPERTIES.contains(&p.key.as_str()) {
        return;
    }
    if LINK_LIST_PROPERTIES.contains(&p.key.as_str()) {
        for item in split_list(&p.value) {
            out.push(Ref::Page {
                name: item,
                kind: RefKind::Property,
            });
        }
        return;
    }
    let mut found = Vec::new();
    scan_refs(&p.value, &mut found);
    for r in found {
        out.push(match r {
            Ref::Page { name, .. } => Ref::Page {
                name,
                kind: RefKind::Property,
            },
            other @ Ref::Block { .. } => other,
        });
    }
}

/// Items of a comma-separated property value, without `[[ ]]` and `#`.
#[must_use]
pub fn split_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' if chars.peek() == Some(&'[') => {
                chars.next();
                depth += 1;
                current.push_str("[[");
            }
            ']' if chars.peek() == Some(&']') && depth > 0 => {
                chars.next();
                depth -= 1;
                current.push_str("]]");
            }
            ',' if depth == 0 => items.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    items.push(current);
    items
        .into_iter()
        .map(|item| {
            let t = item.trim();
            let t = t.strip_prefix('#').unwrap_or(t);
            let t = t
                .strip_prefix("[[")
                .and_then(|x| x.strip_suffix("]]"))
                .unwrap_or(t);
            t.trim().to_string()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Index of the `]]` closing the `[[` at `start`, honouring nesting.
fn closing_brackets(s: &str, start: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i + 1 < b.len() {
        if b[i] == b'[' && b[i + 1] == b'[' {
            depth += 1;
            i += 2;
        } else if b[i] == b']' && b[i + 1] == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += 2;
        } else if b[i] == b'\n' {
            return None;
        } else {
            i += 1;
        }
    }
    None
}

fn tag_stop(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | ';' | '!' | '?' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '`'
        )
}

/// Find `[[page]]`, `#tag`, `#[[tag]]`, `((uuid))` and embeds in a line.
#[allow(clippy::too_many_lines)]
pub fn scan_refs(text: &str, out: &mut Vec<Ref>) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                // Inline code: skip to the closing backtick run.
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                let fence = &text[i..i + run];
                match text[i + run..].find(fence) {
                    Some(end) => i += run + end + run,
                    None => i += run,
                }
            }
            b'{' if text[i..].starts_with("{{") => {
                let Some(end) = text[i..].find("}}") else {
                    i += 2;
                    continue;
                };
                let inner = text[i + 2..i + end].trim();
                let (name, args) = inner.split_once(' ').unwrap_or((inner, ""));
                let args = args.trim();
                if name == "embed" {
                    if let Some(page) = args.strip_prefix("[[").and_then(|a| a.strip_suffix("]]")) {
                        out.push(Ref::Page {
                            name: page.trim().to_string(),
                            kind: RefKind::Embed,
                        });
                    } else if let Some(id) =
                        args.strip_prefix("((").and_then(|a| a.strip_suffix("))"))
                    {
                        if is_uuid(id.trim()) {
                            out.push(Ref::Block {
                                uuid: id.trim().to_ascii_lowercase(),
                                kind: RefKind::Embed,
                            });
                        }
                    }
                    i += end + 2;
                } else if name == "query" {
                    i += end + 2;
                } else {
                    i += 2;
                }
            }
            b'[' if text[i..].starts_with("[[") => match closing_brackets(text, i) {
                Some(end) => {
                    let inner = &text[i + 2..end];
                    if !inner.trim().is_empty() {
                        out.push(Ref::Page {
                            name: inner.trim().to_string(),
                            kind: RefKind::Link,
                        });
                        if inner.contains("[[") {
                            scan_refs(inner, out);
                        }
                    }
                    i = end + 2;
                }
                None => i += 2,
            },
            b'(' if text[i..].starts_with("((") => {
                let id = text.get(i + 2..i + 38);
                if id.is_some_and(is_uuid) && text[i + 38..].starts_with("))") {
                    out.push(Ref::Block {
                        uuid: id.unwrap_or_default().to_ascii_lowercase(),
                        kind: RefKind::BlockRef,
                    });
                    i += 40;
                } else {
                    i += 2;
                }
            }
            b'#' => {
                let at_boundary = i == 0 || text[..i].ends_with(|c: char| c.is_whitespace());
                if !at_boundary {
                    i += 1;
                    continue;
                }
                if text[i + 1..].starts_with("[[") {
                    if let Some(end) = closing_brackets(text, i + 1) {
                        let inner = text[i + 3..end].trim();
                        if !inner.is_empty() {
                            out.push(Ref::Page {
                                name: inner.to_string(),
                                kind: RefKind::Tag,
                            });
                        }
                        i = end + 2;
                        continue;
                    }
                }
                let tail = &text[i + 1..];
                let len = tail.find(tag_stop).unwrap_or(tail.len());
                let tag = tail[..len].trim_end_matches(['.', ':', '\'']);
                if !tag.is_empty() && !tag.starts_with('#') {
                    out.push(Ref::Page {
                        name: tag.to_string(),
                        kind: RefKind::Tag,
                    });
                }
                i += 1 + len;
            }
            _ => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(content: &str) -> Vec<(String, RefKind)> {
        analyze(content)
            .refs
            .into_iter()
            .filter_map(|r| match r {
                Ref::Page { name, kind } => Some((name, kind)),
                Ref::Block { .. } => None,
            })
            .collect()
    }

    #[test]
    fn keys_are_normalised() {
        let p = property_line("Custom_ID Thing:: x");
        assert!(p.is_none(), "keys cannot contain spaces");
        let p = property_line("Background_Color:: red").expect("prop");
        assert_eq!(p.key, "background-color");
        assert_eq!(p.raw_key, "Background_Color");
        assert!(property_line("https://x.test:: y").is_none());
        assert!(property_line("a::b").is_none());
        assert_eq!(property_line("empty::").expect("p").value, "");
    }

    #[test]
    fn marker_priority_heading() {
        let i = analyze(
            "TODO [#A] Ship it\nSCHEDULED: <2026-11-14 Sat .+1w>\nDEADLINE: <2026-11-20 Fri>",
        );
        assert_eq!(i.marker.as_deref(), Some("TODO"));
        assert_eq!(i.priority.as_deref(), Some("A"));
        assert_eq!(i.title, "Ship it");
        assert_eq!(i.scheduled.as_deref(), Some("2026-11-14 Sat .+1w"));
        assert_eq!(i.deadline.as_deref(), Some("2026-11-20 Fri"));
        let h = analyze("## DONE heading");
        assert_eq!((h.heading, h.marker.as_deref()), (Some(2), Some("DONE")));
        assert_eq!(analyze("TODOS are not markers").marker, None);
    }

    #[test]
    fn properties_ids_and_hidden_text() {
        let i = analyze(
            "Block title\nid:: 64F0A1B2-1234-4abc-8def-0123456789ab\ncollapsed:: true\ntype:: [[Book]]\nnot:: a property after text?\n",
        );
        assert_eq!(
            i.uuid().as_deref(),
            Some("64f0a1b2-1234-4abc-8def-0123456789ab")
        );
        assert!(i.collapsed());
        assert_eq!(i.properties.len(), 4);
        assert!(!i.text.contains("id::"));
        assert!(i.text.contains("type:: [[Book]]"));
        let custom = analyze("x\ncustom_id:: 64f0a1b2-1234-4abc-8def-0123456789ab");
        assert!(custom.uuid().is_some());
    }

    #[test]
    fn refs_tags_and_embeds() {
        let got = pages(
            "See [[Page One]] and #tag, #[[multi word]] plus [[outer [[inner]]]] `[[code]]` {{embed [[Emb]]}} x#notag https://a.b/#frag",
        );
        assert_eq!(
            got,
            vec![
                ("Page One".into(), RefKind::Link),
                ("tag".into(), RefKind::Tag),
                ("multi word".into(), RefKind::Tag),
                ("outer [[inner]]".into(), RefKind::Link),
                ("inner".into(), RefKind::Link),
                ("Emb".into(), RefKind::Embed),
            ]
        );
        let b = analyze("ref ((64f0a1b2-1234-4abc-8def-0123456789ab)) {{embed ((64f0a1b2-1234-4abc-8def-0123456789ac))}}");
        assert_eq!(b.refs.len(), 2);
    }

    #[test]
    fn tags_and_alias_values_are_page_refs() {
        let got = pages("tags:: book, [[Science Fiction]], #scifi\nalias:: SF");
        let names: Vec<_> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["book", "Science Fiction", "scifi", "SF"]);
        assert!(got.iter().all(|(_, k)| *k == RefKind::Property));
    }

    #[test]
    fn logbook_drawer_is_skipped() {
        let i = analyze("DOING x\n:LOGBOOK:\nCLOCK: [2026-11-14 Sat 10:00]\n:END:\nafter #t");
        assert!(!i.text.contains("CLOCK"));
        assert_eq!(
            pages("DOING x\n:LOGBOOK:\n[[no]]\n:END:\nafter #t"),
            vec![("t".into(), RefKind::Tag)]
        );
    }

    #[test]
    fn headings_are_not_tags_and_unicode_is_safe() {
        assert_eq!(pages("# Heading"), []);
        assert_eq!(pages("#日本語 x")[0].0, "日本語");
        assert_eq!(pages("#"), []);
    }
}
