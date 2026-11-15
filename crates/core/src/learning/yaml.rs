//! The YAML subset SKILL.md frontmatter uses (agentskills.io): scalars
//! (plain, single- or double-quoted), `|` / `>` block scalars, inline
//! `[a, b]` lists, `- item` lists and nested mappings by indentation.
//! Anchors, tags, multi-document streams and flow mappings are refused.
//! Our own code; Hermes reads the same fields with `PyYAML`.

use std::collections::BTreeMap;

/// A parsed YAML value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Yaml {
    Str(String),
    List(Vec<Yaml>),
    Map(BTreeMap<String, Yaml>),
}

impl Yaml {
    /// The value as text: a scalar, or a list joined by `, `.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Str(s) => Some(s.clone()),
            Self::List(items) => Some(
                items
                    .iter()
                    .filter_map(Self::text)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Map(_) => None,
        }
    }

    /// The value as a list of strings (a scalar becomes one item; a
    /// comma-separated scalar is split).
    #[must_use]
    pub fn strings(&self) -> Vec<String> {
        match self {
            Self::Str(s) if s.trim().is_empty() => Vec::new(),
            Self::Str(s) => s
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect(),
            Self::List(items) => items.iter().filter_map(Self::text).collect(),
            Self::Map(_) => Vec::new(),
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Map(map) => map.get(key),
            _ => None,
        }
    }
}

struct Line<'a> {
    number: usize,
    indent: usize,
    text: &'a str,
}

/// Parse a frontmatter block (the text between the `---` fences).
///
/// # Errors
/// A message naming the first line that is not in the supported subset.
pub fn parse(source: &str) -> Result<Yaml, String> {
    let lines: Vec<Line<'_>> = source
        .lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            let trimmed = raw.trim_end();
            let content = trimmed.trim_start();
            (!content.is_empty() && !content.starts_with('#')).then(|| Line {
                number: index + 1,
                indent: trimmed.len() - content.len(),
                text: content,
            })
        })
        .collect();
    if let Some(tab) = source.lines().position(|l| l.starts_with('\t')) {
        return Err(format!(
            "line {}: tabs are not allowed for indentation",
            tab + 1
        ));
    }
    let raw: Vec<&str> = source.lines().collect();
    let mut cursor = 0;
    let value = block(&lines, &raw, &mut cursor, 0)?;
    if let Some(line) = lines.get(cursor) {
        return Err(format!("line {}: unexpected indentation", line.number));
    }
    Ok(match value {
        Yaml::Str(s) if s.is_empty() => Yaml::Map(BTreeMap::new()),
        other => other,
    })
}

fn block(
    lines: &[Line<'_>],
    raw: &[&str],
    cursor: &mut usize,
    indent: usize,
) -> Result<Yaml, String> {
    let Some(first) = lines.get(*cursor) else {
        return Ok(Yaml::Str(String::new()));
    };
    if first.indent < indent {
        return Ok(Yaml::Str(String::new()));
    }
    let indent = first.indent;
    if first.text.starts_with("- ") || first.text == "-" {
        let mut items = Vec::new();
        while let Some(line) = lines.get(*cursor) {
            if line.indent != indent || !(line.text.starts_with("- ") || line.text == "-") {
                break;
            }
            let rest = line.text.strip_prefix('-').unwrap_or_default().trim();
            *cursor += 1;
            if rest.is_empty() {
                items.push(block(lines, raw, cursor, indent + 1)?);
            } else if rest.contains(": ") && !is_quoted(rest) {
                return Err(format!(
                    "line {}: mappings inside lists are not supported",
                    line.number
                ));
            } else {
                items.push(scalar_or_inline(rest, line.number)?);
            }
        }
        return Ok(Yaml::List(items));
    }
    let mut map = BTreeMap::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(format!("line {}: unexpected indentation", line.number));
        }
        let (key, rest) = split_key(line.text)
            .ok_or_else(|| format!("line {}: expected `key: value`", line.number))?;
        *cursor += 1;
        let value = match rest {
            "" => {
                // A list may sit at the key's own indentation.
                let nested_list = lines.get(*cursor).is_some_and(|next| {
                    next.indent == indent && (next.text.starts_with("- ") || next.text == "-")
                });
                if nested_list {
                    block(lines, raw, cursor, indent)?
                } else {
                    block(lines, raw, cursor, indent + 1)?
                }
            }
            "|" | "|-" | "|+" | ">" | ">-" | ">+" => block_scalar(lines, raw, cursor, indent, rest),
            other => scalar_or_inline(other, line.number)?,
        };
        if map.insert(key.clone(), value).is_some() {
            return Err(format!("line {}: duplicate key `{key}`", line.number));
        }
    }
    Ok(Yaml::Map(map))
}

/// `|` keeps line breaks, `>` folds them into spaces. The body is every
/// following line indented deeper than the key (blank lines included).
fn block_scalar(
    lines: &[Line<'_>],
    raw: &[&str],
    cursor: &mut usize,
    indent: usize,
    style: &str,
) -> Yaml {
    let Some(start) = lines.get(*cursor).filter(|l| l.indent > indent) else {
        return Yaml::Str(String::new());
    };
    let body_indent = start.indent;
    let first_raw = start.number - 1;
    let mut last_raw = first_raw;
    while let Some(line) = lines.get(*cursor) {
        if line.indent <= indent {
            break;
        }
        last_raw = line.number - 1;
        *cursor += 1;
    }
    let body: Vec<String> = raw[first_raw..=last_raw]
        .iter()
        .map(|l| {
            l.get(body_indent.min(l.len())..)
                .unwrap_or("")
                .trim_end()
                .to_string()
        })
        .collect();
    let text = if style.starts_with('|') {
        body.join("\n")
    } else {
        body.split(String::is_empty)
            .map(|para| para.join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Yaml::Str(text)
}

fn split_key(text: &str) -> Option<(String, &str)> {
    if let Some(quote) = text.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let end = text[1..].find(quote)? + 1;
        let key = text[1..end].to_string();
        let rest = text[end + 1..].trim_start().strip_prefix(':')?;
        return Some((key, strip_comment(rest.trim())));
    }
    let (key, rest) = text.split_once(':')?;
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let key = key.trim();
    if key.is_empty() || key.starts_with(['&', '*', '!', '?', '{', '[']) {
        return None;
    }
    Some((key.to_string(), strip_comment(rest.trim())))
}

fn is_quoted(text: &str) -> bool {
    (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
}

/// Drop a ` # comment` after an unquoted value.
fn strip_comment(value: &str) -> &str {
    if value.starts_with(['"', '\'']) {
        return value;
    }
    value.find(" #").map_or(value, |at| value[..at].trim_end())
}

fn scalar_or_inline(text: &str, number: usize) -> Result<Yaml, String> {
    let text = strip_comment(text.trim());
    if text.starts_with(['&', '*', '!']) {
        return Err(format!(
            "line {number}: anchors, aliases and tags are not supported"
        ));
    }
    if text.starts_with('{') {
        return Err(format!("line {number}: flow mappings are not supported"));
    }
    if let Some(inner) = text.strip_prefix('[') {
        let inner = inner
            .strip_suffix(']')
            .ok_or_else(|| format!("line {number}: unclosed `[`"))?;
        let items = split_flow(inner)
            .into_iter()
            .filter(|item| !item.trim().is_empty())
            .map(|item| Yaml::Str(unquote(item.trim())))
            .collect();
        return Ok(Yaml::List(items));
    }
    Ok(Yaml::Str(unquote(text)))
}

/// Split `a, "b, c", d` on commas outside quotes.
fn split_flow(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, c) in inner.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ',') => {
                parts.push(&inner[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        let inner = &text[1..text.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if text.len() >= 2 && text.starts_with('\'') && text.ends_with('\'') {
        return text[1..text.len() - 1].replace("''", "'");
    }
    text.to_string()
}

#[cfg(test)]
#[allow(clippy::assert_is_empty, clippy::bool_assert_comparison)]
mod tests {
    use super::*;

    fn s(v: &str) -> Yaml {
        Yaml::Str(v.into())
    }

    #[test]
    fn scalars_lists_and_nested_maps() {
        let doc = parse(
            "name: pdf-tools\ndescription: \"Fill PDFs: forms, too\"\nversion: 1.0.0 # comment\nplatforms: [macos, linux]\nmetadata:\n  tags:\n    - pdf\n    - 'forms'\n  category: documents\n",
        )
        .expect("parse");
        assert_eq!(doc.get("name"), Some(&s("pdf-tools")));
        assert_eq!(doc.get("description"), Some(&s("Fill PDFs: forms, too")));
        assert_eq!(doc.get("version"), Some(&s("1.0.0")));
        assert_eq!(
            doc.get("platforms").map(Yaml::strings),
            Some(vec!["macos".to_string(), "linux".to_string()])
        );
        let meta = doc.get("metadata").expect("metadata");
        assert_eq!(meta.get("category"), Some(&s("documents")));
        assert_eq!(
            meta.get("tags").map(Yaml::strings),
            Some(vec!["pdf".to_string(), "forms".to_string()])
        );
    }

    #[test]
    fn block_scalars_keep_or_fold_lines() {
        let doc = parse("a: |\n  one\n  two\nb: >\n  three\n  four\nc: x\n").expect("parse");
        assert_eq!(doc.get("a"), Some(&s("one\ntwo")));
        assert_eq!(doc.get("b"), Some(&s("three four")));
        assert_eq!(doc.get("c"), Some(&s("x")));
    }

    #[test]
    fn list_at_key_indentation() {
        let doc = parse("tags:\n- a\n- b\n").expect("parse");
        assert_eq!(
            doc.get("tags").map(Yaml::strings),
            Some(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn refuses_unsupported_syntax() {
        assert!(parse("a: &x 1\n").is_err());
        assert!(parse("a: {b: 1}\n").is_err());
        assert!(parse("a: 1\na: 2\n").is_err());
        assert!(parse("just text\n").is_err());
        assert!(parse("a:\n\tb: 1\n").is_err());
        assert!(parse("a: 1\n   b: 2\n").is_err());
    }

    #[test]
    fn empty_frontmatter_is_an_empty_map() {
        assert_eq!(parse("").expect("parse"), Yaml::Map(BTreeMap::new()));
        assert_eq!(
            parse("# only a comment\n").expect("parse"),
            Yaml::Map(BTreeMap::new())
        );
    }
}
