// Adapted from Onlook (Apache-2.0, © 2024 On Off, Inc.):
// packages/parser/src/code-edit/transform.ts and code-edit/style.ts
// @ 423e2e9 — modified: instead of a Babel AST pass that merges Tailwind
// classes into `className`, this finds the element's opening tag at the
// reported source position and writes the style change as Svelte
// `style:` directives, a merged static `style="…"` (Vue / HTML) or a merged
// `style={{…}}` object (JSX). No formatter runs; untouched text is kept.
//! Write a visual style edit back into the source of one element.

use crate::DesignError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How a file spells element styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Syntax {
    Svelte,
    Vue,
    Html,
    Jsx,
}

impl Syntax {
    /// From a file extension.
    #[must_use]
    pub fn for_path(path: &str) -> Option<Self> {
        let ext = Path::new(path)
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase();
        match ext.as_str() {
            "svelte" => Some(Self::Svelte),
            "vue" => Some(Self::Vue),
            "html" | "astro" => Some(Self::Html),
            "jsx" | "tsx" | "js" | "ts" | "mjs" => Some(Self::Jsx),
            _ => None,
        }
    }
}

/// One CSS property change; an empty value removes the property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleChange {
    pub property: String,
    pub value: String,
}

/// Where an opening tag sits in the source (byte offsets, `end` exclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningTag {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// The result of an edit: the whole new file plus the changed tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleEdit {
    pub before: String,
    pub after: String,
    pub contents: String,
}

/// Validate a CSS property name (`margin-top`, `--accent`).
///
/// # Errors
/// [`DesignError::BadStyle`] for anything else.
pub fn check_property(name: &str) -> Result<(), DesignError> {
    let body = name.strip_prefix("--").unwrap_or(name);
    let ok = !body.is_empty()
        && body.len() <= 64
        && body
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !body.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(DesignError::BadStyle(format!("property `{name}`")))
    }
}

/// Validate a CSS value: no characters that could end the attribute, the
/// declaration or the tag (`"`, `'`, `` ` ``, `;`, `{`, `}`, `<`, `>`, `\`,
/// newlines) and no `url(` / `expression(` (edits never add network loads).
///
/// # Errors
/// [`DesignError::BadStyle`].
pub fn check_value(value: &str) -> Result<(), DesignError> {
    let lower = value.to_ascii_lowercase();
    let bad = value.len() > 200
        || value.chars().any(|c| {
            matches!(c, '"' | '\'' | '`' | ';' | '{' | '}' | '<' | '>' | '\\') || c.is_control()
        })
        || lower.contains("url(")
        || lower.contains("expression(")
        || lower.contains("@import");
    if bad {
        Err(DesignError::BadStyle(format!("value `{value}`")))
    } else {
        Ok(())
    }
}

/// Byte offset of a 1-based line and column (column 0 → line start).
fn offset_of(src: &str, line: u32, column: u32) -> usize {
    let mut offset = 0;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line as usize {
            let col = column.saturating_sub(1) as usize;
            return offset + l.char_indices().nth(col).map_or(l.len(), |(b, _)| b);
        }
        offset += l.len();
    }
    src.len()
}

fn is_tag_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b':' | b'_')
}

/// Find the opening tag of the element reported at `line:column` with tag
/// name `hint` (empty = any). The nearest matching `<tag` to the position
/// wins, preferring one at or after it, so off-by-one columns from different
/// frameworks still land on the right element.
///
/// # Errors
/// [`DesignError::NotFound`] when no tag matches.
pub fn find_opening_tag(
    src: &str,
    line: u32,
    column: u32,
    hint: &str,
) -> Result<OpeningTag, DesignError> {
    let target = offset_of(src, line, column);
    let bytes = src.as_bytes();
    let mut best: Option<(usize, usize)> = None; // (distance, start)
    let mut i = 0;
    while let Some(pos) = src[i..].find('<') {
        let start = i + pos;
        i = start + 1;
        let name_len = bytes[start + 1..]
            .iter()
            .take_while(|b| is_tag_char(**b))
            .count();
        if name_len == 0 || !bytes[start + 1].is_ascii_alphabetic() {
            continue;
        }
        let name = &src[start + 1..start + 1 + name_len];
        if !hint.is_empty() && !name.eq_ignore_ascii_case(hint) {
            continue;
        }
        let distance = if start >= target {
            start - target
        } else {
            (target - start) * 2 + 1
        };
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, start));
        }
    }
    let (distance, start) =
        best.ok_or_else(|| DesignError::NotFound(format!("<{hint}> near line {line}")))?;
    // More than ~40 lines away means the location is stale.
    if distance > 4000 {
        return Err(DesignError::NotFound(format!("<{hint}> near line {line}")));
    }
    let end = scan_tag_end(src, start)
        .ok_or_else(|| DesignError::NotFound("end of the opening tag".into()))?;
    let name_len = bytes[start + 1..]
        .iter()
        .take_while(|b| is_tag_char(**b))
        .count();
    Ok(OpeningTag {
        start,
        end,
        name: src[start + 1..start + 1 + name_len].to_string(),
    })
}

/// The byte after the `>` closing the tag that starts at `start`, skipping
/// quoted values and `{…}` expressions (Svelte, JSX).
fn scan_tag_end(src: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    for (i, ch) in src[start + 1..].char_indices() {
        let at = start + 1 + i;
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '>' if depth == 0 => return Some(at + 1),
            '<' if depth == 0 => return None, // malformed
            _ => {}
        }
    }
    None
}

/// One attribute inside an opening tag (offsets relative to the tag text).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attr {
    name: String,
    start: usize,
    end: usize,
    /// Raw value including its quotes or braces.
    value: Option<(usize, usize)>,
}

fn attributes(tag: &str) -> Vec<Attr> {
    let bytes = tag.as_bytes();
    let mut i = 1 + bytes[1..].iter().take_while(|b| is_tag_char(**b)).count();
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() || c == b'/' || c == b'>' {
            i += 1;
            continue;
        }
        if c == b'{' {
            // Svelte `{...spread}` / JSX `{...props}`.
            i = skip_braces(tag, i);
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name = tag[start..i].to_string();
        let mut value = None;
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            let vstart = i;
            match bytes.get(i) {
                Some(b'"' | b'\'') => {
                    let q = bytes[i];
                    i += 1 + tag[i + 1..]
                        .bytes()
                        .position(|b| b == q)
                        .map_or(tag.len() - i - 1, |p| p + 1);
                }
                Some(b'{') => i = skip_braces(tag, i),
                _ => {
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                }
            }
            value = Some((vstart, i));
        }
        if !name.is_empty() {
            out.push(Attr {
                name,
                start,
                end: i,
                value,
            });
        }
    }
    out
}

/// Index just past the `}` matching the `{` at `i`, honouring quotes.
fn skip_braces(s: &str, i: usize) -> usize {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (j, ch) in s[i..].char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + j + 1;
                }
            }
            _ => {}
        }
    }
    s.len()
}

/// Where new attributes go: before `/>` or `>`.
fn insert_point(tag: &str) -> usize {
    let trimmed = tag.trim_end_matches('>');
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    trimmed.trim_end().len()
}

/// Apply `changes` to the element at `line:column` (`tag` name hint) in
/// `src`, written for `syntax`.
///
/// # Errors
/// [`DesignError::BadStyle`] for unsafe properties or values,
/// [`DesignError::NotFound`] when the element can't be located, and
/// [`DesignError::Unsupported`] for a dynamic `style` the edit can't merge.
pub fn apply_style(
    src: &str,
    syntax: Syntax,
    line: u32,
    column: u32,
    tag: &str,
    changes: &[StyleChange],
) -> Result<StyleEdit, DesignError> {
    if changes.is_empty() {
        return Err(DesignError::BadStyle("no changes".into()));
    }
    for c in changes {
        check_property(&c.property)?;
        if !c.value.is_empty() {
            check_value(&c.value)?;
        }
    }
    let open = find_opening_tag(src, line, column, tag)?;
    let before = &src[open.start..open.end];
    let after = match syntax {
        Syntax::Svelte => svelte_directives(before, changes),
        Syntax::Vue | Syntax::Html => static_style(before, changes)?,
        Syntax::Jsx => jsx_style(before, changes)?,
    };
    let mut contents = String::with_capacity(src.len() + after.len());
    contents.push_str(&src[..open.start]);
    contents.push_str(&after);
    contents.push_str(&src[open.end..]);
    Ok(StyleEdit {
        before: before.to_string(),
        after,
        contents,
    })
}

fn svelte_directives(tag: &str, changes: &[StyleChange]) -> String {
    let mut out = tag.to_string();
    for change in changes {
        let attrs = attributes(&out);
        let name = format!("style:{}", change.property);
        let existing = attrs
            .iter()
            .find(|a| a.name == name || a.name.starts_with(&format!("{name}|")));
        match (existing, change.value.is_empty()) {
            (Some(a), true) => {
                // Remove the directive and the whitespace before it.
                let lead = out[..a.start].trim_end().len();
                out.replace_range(lead..a.end, "");
            }
            (Some(a), false) => {
                out.replace_range(a.start..a.end, &format!("{name}=\"{}\"", change.value));
            }
            (None, true) => {}
            (None, false) => {
                let at = insert_point(&out);
                out.insert_str(at, &format!(" {name}=\"{}\"", change.value));
            }
        }
    }
    out
}

/// Merge into `style="a: b; c: d"` (creating it when missing).
fn static_style(tag: &str, changes: &[StyleChange]) -> Result<String, DesignError> {
    let attrs = attributes(tag);
    let existing = attrs.iter().find(|a| a.name.eq_ignore_ascii_case("style"));
    let mut decls: Vec<(String, String)> = Vec::new();
    if let Some(a) = existing {
        let (vs, ve) = a
            .value
            .ok_or_else(|| DesignError::Unsupported("empty style attribute".into()))?;
        let raw = &tag[vs..ve];
        let inner = raw
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .or_else(|| raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
            .ok_or_else(|| DesignError::Unsupported("a dynamic style attribute".into()))?;
        if inner.contains('{') {
            return Err(DesignError::Unsupported(
                "a style attribute with expressions".into(),
            ));
        }
        for part in inner.split(';') {
            if let Some((k, v)) = part.split_once(':') {
                decls.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    for change in changes {
        let pos = decls
            .iter()
            .position(|(k, _)| k.eq_ignore_ascii_case(&change.property));
        match (pos, change.value.is_empty()) {
            (Some(i), true) => {
                decls.remove(i);
            }
            (Some(i), false) => decls[i].1.clone_from(&change.value),
            (None, true) => {}
            (None, false) => decls.push((change.property.clone(), change.value.clone())),
        }
    }
    let body = decls
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let mut out = tag.to_string();
    match (existing, body.is_empty()) {
        (Some(a), true) => {
            let lead = out[..a.start].trim_end().len();
            out.replace_range(lead..a.end, "");
        }
        (Some(a), false) => out.replace_range(a.start..a.end, &format!("style=\"{body}\"")),
        (None, true) => {}
        (None, false) => {
            let at = insert_point(&out);
            out.insert_str(at, &format!(" style=\"{body}\""));
        }
    }
    Ok(out)
}

/// `margin-top` → `marginTop`; custom properties stay quoted keys.
fn jsx_key(property: &str) -> String {
    if property.starts_with("--") {
        return format!("\"{property}\"");
    }
    let mut out = String::new();
    let mut upper = false;
    for c in property.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Split an object literal body into top-level `key: value` entries.
fn object_entries(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for ch in body.chars() {
        if let Some(q) = quote {
            cur.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => {
                quote = Some(ch);
                cur.push(ch);
            }
            '{' | '(' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ')' | ']' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn entry_key(entry: &str) -> Option<String> {
    let (k, _) = entry.split_once(':')?;
    let k = k.trim().trim_matches(|c| c == '"' || c == '\'');
    (!k.starts_with("...")).then(|| k.to_string())
}

fn jsx_style(tag: &str, changes: &[StyleChange]) -> Result<String, DesignError> {
    let attrs = attributes(tag);
    let existing = attrs.iter().find(|a| a.name == "style");
    let mut entries: Vec<String> = Vec::new();
    if let Some(a) = existing {
        let (vs, ve) = a
            .value
            .ok_or_else(|| DesignError::Unsupported("empty style prop".into()))?;
        let raw = tag[vs..ve].trim();
        let expr = raw
            .strip_prefix('{')
            .and_then(|r| r.strip_suffix('}'))
            .ok_or_else(|| DesignError::Unsupported("a string style prop".into()))?
            .trim();
        match expr.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            Some(body) => entries = object_entries(body),
            None => entries.push(format!("...({expr})")),
        }
    }
    for change in changes {
        let key = jsx_key(&change.property);
        let bare = key.trim_matches('"').to_string();
        let pos = entries
            .iter()
            .position(|e| entry_key(e).as_deref() == Some(bare.as_str()));
        let entry = format!(
            "{key}: {}",
            serde_json::to_string(&change.value).unwrap_or_default()
        );
        match (pos, change.value.is_empty()) {
            (Some(i), true) => {
                entries.remove(i);
            }
            (Some(i), false) => entries[i] = entry,
            (None, true) => {}
            (None, false) => entries.push(entry),
        }
    }
    let mut out = tag.to_string();
    let rendered = format!("style={{{{ {} }}}}", entries.join(", "));
    match (existing, entries.is_empty()) {
        (Some(a), true) => {
            let lead = out[..a.start].trim_end().len();
            out.replace_range(lead..a.end, "");
        }
        (Some(a), false) => out.replace_range(a.start..a.end, &rendered),
        (None, true) => {}
        (None, false) => {
            let at = insert_point(&out);
            out.insert_str(at, &format!(" {rendered}"));
        }
    }
    Ok(out)
}

/// Re-find the edited tag in another copy of the file (the task worktree's,
/// which may differ from the dev server's working copy): the `n`-th exact
/// occurrence of the original tag text, where `n` is its index in `seen`.
///
/// # Errors
/// [`DesignError::NotFound`] when the copies disagree about that tag.
pub fn transplant(seen: &str, target: &str, edit: &StyleEdit) -> Result<String, DesignError> {
    if seen == target {
        return Ok(edit.contents.clone());
    }
    let at = edit_offset(seen, edit).ok_or_else(|| DesignError::NotFound("edited tag".into()))?;
    let nth = seen[..at].matches(edit.before.as_str()).count();
    let pos = target
        .match_indices(edit.before.as_str())
        .nth(nth)
        .map(|(i, _)| i)
        .ok_or_else(|| {
            DesignError::NotFound(
                "the element in the committed file (commit or stash local changes to this file first)".into(),
            )
        })?;
    let mut out = target.to_string();
    out.replace_range(pos..pos + edit.before.len(), &edit.after);
    Ok(out)
}

/// Where `edit.before` was replaced in `seen` (first differing byte, walked
/// back to the tag start).
fn edit_offset(seen: &str, edit: &StyleEdit) -> Option<usize> {
    let diff = seen
        .bytes()
        .zip(edit.contents.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(seen.len().min(edit.contents.len()));
    seen.match_indices(edit.before.as_str())
        .map(|(i, _)| i)
        .filter(|i| *i <= diff && diff <= i + edit.before.len())
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(p: &str, v: &str) -> StyleChange {
        StyleChange {
            property: p.into(),
            value: v.into(),
        }
    }

    #[test]
    fn syntax_from_path() {
        assert_eq!(Syntax::for_path("a/B.svelte"), Some(Syntax::Svelte));
        assert_eq!(Syntax::for_path("B.tsx"), Some(Syntax::Jsx));
        assert_eq!(Syntax::for_path("B.vue"), Some(Syntax::Vue));
        assert_eq!(Syntax::for_path("B.md"), None);
    }

    #[test]
    fn svelte_adds_replaces_and_removes_directives() {
        let src = "<script>let n = 1;</script>\n<div class=\"card\" onclick={() => n > 1}>\n  <h1 style:color=\"red\">Hi</h1>\n</div>\n";
        let e = apply_style(
            src,
            Syntax::Svelte,
            2,
            1,
            "div",
            &[ch("padding", "12px"), ch("gap", "var(--space-2)")],
        )
        .expect("edit");
        assert_eq!(e.after, "<div class=\"card\" onclick={() => n > 1} style:padding=\"12px\" style:gap=\"var(--space-2)\">");
        assert!(e.contents.contains("<h1 style:color=\"red\">"));
        let h = apply_style(
            src,
            Syntax::Svelte,
            3,
            3,
            "h1",
            &[ch("color", "var(--accent)")],
        )
        .expect("edit");
        assert_eq!(h.after, "<h1 style:color=\"var(--accent)\">");
        let r = apply_style(src, Syntax::Svelte, 3, 3, "h1", &[ch("color", "")]).expect("edit");
        assert_eq!(r.after, "<h1>");
    }

    #[test]
    fn html_merges_static_style() {
        let src = "<p style=\"color: red; margin: 0\">x</p><img src=\"a.png\" />";
        let e = apply_style(
            src,
            Syntax::Html,
            1,
            1,
            "p",
            &[
                ch("color", "blue"),
                ch("margin", ""),
                ch("font-size", "14px"),
            ],
        )
        .expect("edit");
        assert_eq!(e.after, "<p style=\"color: blue; font-size: 14px\">");
        let i = apply_style(src, Syntax::Vue, 1, 40, "img", &[ch("width", "10px")]).expect("edit");
        assert_eq!(i.after, "<img src=\"a.png\" style=\"width: 10px\" />");
        let dynamic = "<p style={x}>";
        assert!(matches!(
            apply_style(dynamic, Syntax::Html, 1, 1, "p", &[ch("color", "red")]),
            Err(DesignError::Unsupported(_))
        ));
    }

    #[test]
    fn jsx_merges_style_objects() {
        let src = "export const A = () => (\n  <div className=\"a\" style={{ marginTop: 4, color: theme.fg }}>\n    <span>{a > b ? 'x' : 'y'}</span>\n  </div>\n);\n";
        let e = apply_style(
            src,
            Syntax::Jsx,
            2,
            3,
            "div",
            &[
                ch("margin-top", "8px"),
                ch("--accent", "red"),
                ch("color", ""),
            ],
        )
        .expect("edit");
        assert_eq!(
            e.after,
            "<div className=\"a\" style={{ marginTop: \"8px\", \"--accent\": \"red\" }}>"
        );
        let s =
            apply_style(src, Syntax::Jsx, 3, 5, "span", &[ch("display", "flex")]).expect("edit");
        assert_eq!(s.after, "<span style={{ display: \"flex\" }}>");
        let spread = "<b style={base}/>";
        let b = apply_style(spread, Syntax::Jsx, 1, 1, "b", &[ch("color", "red")]).expect("edit");
        assert_eq!(b.after, "<b style={{ ...(base), color: \"red\" }}/>");
    }

    #[test]
    fn refuses_unsafe_values_and_names() {
        let src = "<p>x</p>";
        for bad in [
            "red\" onclick=\"x",
            "red; background: url(http://x)",
            "a}",
            "url(x)",
            "</style>",
            "a\nb",
        ] {
            assert!(
                matches!(
                    apply_style(src, Syntax::Html, 1, 1, "p", &[ch("color", bad)]),
                    Err(DesignError::BadStyle(_))
                ),
                "{bad}"
            );
        }
        for bad in ["Color", "on-click=", "-x", "", "a b"] {
            assert!(check_property(bad).is_err(), "{bad}");
        }
        assert!(check_property("--brand-1").is_ok());
    }

    #[test]
    fn finds_nearest_tag_with_offset_columns() {
        let src = "<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>";
        // Column points at the indentation, React-style 1-based.
        let t = find_opening_tag(src, 3, 1, "li").expect("li");
        assert_eq!(&src[t.start..t.end], "<li>");
        assert_eq!(t.start, src.rfind("<li>").expect("pos"));
        assert!(find_opening_tag(src, 1, 1, "table").is_err());
    }

    #[test]
    fn transplants_into_a_diverged_copy() {
        let seen = "<p>a</p>\n<p>b</p>\n";
        let e = apply_style(seen, Syntax::Html, 2, 1, "p", &[ch("color", "red")]).expect("edit");
        let committed = "<!-- new header -->\n<p>a</p>\n<p>b</p>\n";
        let out = transplant(seen, committed, &e).expect("ok");
        assert_eq!(
            out,
            "<!-- new header -->\n<p>a</p>\n<p style=\"color: red\">b</p>\n"
        );
        assert!(transplant(seen, "<div/>", &e).is_err());
        assert_eq!(transplant(seen, seen, &e).expect("same"), e.contents);
    }
}
