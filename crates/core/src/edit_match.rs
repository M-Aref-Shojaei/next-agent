// Ported from opencode (MIT, © 2025 opencode): packages/core/src/tool/plugin/edit.ts @ cbe71595e1f370ff91b3081872b96bbe56519c41
//! Tiered find-and-replace for the `edit` tool and the `apply_patch`
//! fallback.
//!
//! Matching runs on the LF view of the file (what `read_file` shows the
//! model) and maps positions back to the original bytes, so CRLF and mixed
//! line endings neither block matches nor leak into untouched regions. The
//! first tier that finds anything wins:
//! 1. exact match;
//! 2. Unicode-normalised: typographic quotes, dashes and non-breaking spaces
//!    compare as their ASCII forms (a one-to-one character mapping, so
//!    positions survive);
//! 3. line-based, ignoring trailing whitespace on each line.
//!
//! A replacement keeps the line endings of the region it replaces: lines the
//! replacement keeps (longest common subsequence) reuse their ending, changed
//! lines reuse the ending of the line they replace, and only new lines take
//! the region's (or file's) dominant ending. A leading UTF-8 BOM is preserved
//! by [`split_bom`] / [`join_bom`].
//!
//! Pure: no I/O. `LocalTools` reads and writes the file.

use std::collections::HashMap;

const CRLF: &str = "\r\n";
const BOM: char = '\u{feff}';
/// Lines shown in a miss hint.
const HINT_LINES: usize = 12;
/// Characters per hint line.
const HINT_LINE_LENGTH: usize = 200;
/// Beyond this many LCS cells, line endings are aligned by position.
const MAX_ALIGN_CELLS: usize = 4_000_000;

/// Which tier produced the matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Exact,
    Unicode,
    TrailingWhitespace,
    /// `old` and `new` differ only in `\r`: applied to the exact bytes.
    LineEndings,
}

/// A byte range in the original text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

/// A successful edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited {
    pub text: String,
    pub replacements: usize,
    pub tier: Tier,
}

/// Why an edit could not be applied. `Display` is the message the model sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    EmptyOld,
    Identical,
    NotFound { hint: Option<String> },
    Multiple { count: usize },
    Unchanged { endings_only: bool },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyOld => f.write_str(
                "old_string must not be empty. Use write_file to create or overwrite a file.",
            ),
            Self::Identical => {
                f.write_str("No changes to apply: old_string and new_string are identical.")
            }
            Self::NotFound { hint } => {
                f.write_str(
                    "Could not find old_string in the file. It must match exactly, including whitespace and indentation.",
                )?;
                if let Some(hint) = hint {
                    write!(f, "\n\n{hint}")?;
                }
                Ok(())
            }
            Self::Multiple { count } => write!(
                f,
                "Found {count} matches for old_string, but expected exactly one. Add more surrounding context to make old_string unique, or set replace_all to true to replace every occurrence."
            ),
            Self::Unchanged { endings_only: true } => {
                f.write_str("No changes to apply: the file already has those line endings.")
            }
            Self::Unchanged { endings_only: false } => f.write_str(
                "No changes to apply: the edit leaves the file unchanged. Line endings follow the file; to change them, write \\r explicitly in old_string or new_string.",
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// Split a leading UTF-8 BOM off `text`.
#[must_use]
pub fn split_bom(text: &str) -> (bool, &str) {
    match text.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, text),
    }
}

/// Re-attach a BOM (at most one).
#[must_use]
pub fn join_bom(bom: bool, text: &str) -> String {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    if bom {
        format!("{BOM}{text}")
    } else {
        text.to_string()
    }
}

/// Replace `old` with `new` in `source` (BOM already removed).
///
/// # Errors
/// See [`EditError`]: empty/identical input, no match (with a hint), more
/// than one match without `replace_all`, or an edit that changes nothing.
pub fn apply_edit(
    source: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<Edited, EditError> {
    if old == new {
        return Err(EditError::Identical);
    }
    if old.is_empty() {
        return Err(EditError::EmptyOld);
    }
    let found = find(source, old, new);
    if found.matches.is_empty() {
        return Err(EditError::NotFound {
            hint: miss_hint(source, old, new),
        });
    }
    if found.matches.len() > 1 && !replace_all {
        return Err(EditError::Multiple {
            count: found.matches.len(),
        });
    }
    let chosen: Vec<Match> = if replace_all {
        found.matches.clone()
    } else {
        found.matches[..1].to_vec()
    };
    let text = replace_matches(source, &chosen, |region| found.render(region, new));
    if text == source {
        return Err(EditError::Unchanged {
            endings_only: found.tier == Tier::LineEndings,
        });
    }
    Ok(Edited {
        text,
        replacements: chosen.len(),
        tier: found.tier,
    })
}

/// Matches for `old` in `source`, plus how to render a replacement.
#[derive(Debug, Clone)]
pub struct Found {
    pub matches: Vec<Match>,
    pub tier: Tier,
    file_ending: &'static str,
}

impl Found {
    /// Replacement text for one matched `region` (original bytes).
    #[must_use]
    pub fn render(&self, region: &str, new: &str) -> String {
        if self.tier == Tier::LineEndings {
            return new.to_string();
        }
        let replacement = new.replace(CRLF, "\n");
        with_region_endings(region, &replacement, line_ending(region, self.file_ending))
    }
}

/// Find `old` in `source` with the tiered strategy (non-overlapping matches,
/// in order). `new` decides whether this is a line-endings-only edit.
#[must_use]
pub fn find(source: &str, old: &str, new: &str) -> Found {
    let file_ending = line_ending(source, "\n");
    if old.is_empty() {
        return Found {
            matches: Vec::new(),
            tier: Tier::Exact,
            file_ending,
        };
    }
    if old.replace('\r', "") == new.replace('\r', "") {
        let src: Vec<char> = source.chars().collect();
        let pat: Vec<char> = old.chars().collect();
        let bytes = byte_offsets(source);
        let matches = find_occurrences(&src, &pat)
            .into_iter()
            .map(|(start, end)| Match {
                start: bytes[start],
                end: bytes[end],
            })
            .collect();
        return Found {
            matches,
            tier: Tier::LineEndings,
            file_ending,
        };
    }

    // The LF view, plus where each removed `\r` sat (in view coordinates).
    let mut view: Vec<char> = Vec::with_capacity(source.len());
    let mut removed: Vec<usize> = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && chars.peek() == Some(&'\n') {
            removed.push(view.len());
            continue;
        }
        view.push(c);
    }
    let search: Vec<char> = old.replace(CRLF, "\n").chars().collect();

    let mut tier = Tier::Exact;
    let mut hits = find_occurrences(&view, &search);
    if hits.is_empty() {
        let norm_view: Vec<char> = view.iter().copied().map(normalize_char).collect();
        let norm_search: Vec<char> = search.iter().copied().map(normalize_char).collect();
        hits = find_occurrences(&norm_view, &norm_search);
        tier = Tier::Unicode;
    }
    if hits.is_empty() {
        hits = find_line_occurrences(&view, &search);
        tier = Tier::TrailingWhitespace;
    }
    let bytes = byte_offsets(source);
    let to_source = |index: usize| bytes[index + count_before(&removed, index)];
    Found {
        matches: hits
            .into_iter()
            .map(|(start, end)| Match {
                start: to_source(start),
                end: to_source(end),
            })
            .collect(),
        tier,
        file_ending,
    }
}

/// Apply replacements last to first so earlier offsets stay valid.
#[must_use]
pub fn replace_matches(
    source: &str,
    matches: &[Match],
    mut render: impl FnMut(&str) -> String,
) -> String {
    let mut out = source.to_string();
    for m in matches.iter().rev() {
        let replacement = render(&source[m.start..m.end]);
        out.replace_range(m.start..m.end, &replacement);
    }
    out
}

/// The dominant line ending in `text`, or `fallback` on a tie.
#[must_use]
pub fn line_ending(text: &str, fallback: &'static str) -> &'static str {
    let crlf = text.matches(CRLF).count();
    let lf = text.matches('\n').count() - crlf;
    match crlf.cmp(&lf) {
        std::cmp::Ordering::Equal => fallback,
        std::cmp::Ordering::Greater => CRLF,
        std::cmp::Ordering::Less => "\n",
    }
}

/// 1-based line number of byte offset `at`.
#[must_use]
pub fn line_of(source: &str, at: usize) -> usize {
    source[..at.min(source.len())].matches('\n').count() + 1
}

fn normalize_char(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
        other => other,
    }
}

fn normalize(text: &str) -> String {
    text.chars().map(normalize_char).collect()
}

/// Byte offset of every char index (plus the end).
fn byte_offsets(text: &str) -> Vec<usize> {
    let mut out: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    out.push(text.len());
    out
}

fn find_occurrences(content: &[char], search: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if search.is_empty() || search.len() > content.len() {
        return out;
    }
    let mut offset = 0;
    while offset + search.len() <= content.len() {
        if content[offset..offset + search.len()] == *search {
            out.push((offset, offset + search.len()));
            offset += search.len();
        } else {
            offset += 1;
        }
    }
    out
}

struct Line {
    start: usize,
    end: usize,
    text: String,
    content_end: usize,
    newline: bool,
}

fn view_lines(view: &[char]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < view.len() {
        let mut end = start;
        while end < view.len() && view[end] != '\n' {
            end += 1;
        }
        let newline = end < view.len();
        let text: String = view[start..end].iter().collect();
        let content_end = if text.ends_with('\r') { end - 1 } else { end };
        lines.push(Line {
            start,
            end: if newline { end + 1 } else { end },
            text,
            content_end,
            newline,
        });
        start = if newline { end + 1 } else { end };
    }
    lines
}

fn find_line_occurrences(view: &[char], search: &[char]) -> Vec<(usize, usize)> {
    let search: String = search.iter().collect();
    let trailing_newline = search.ends_with('\n');
    let mut expected: Vec<&str> = search.split('\n').collect();
    if trailing_newline {
        expected.pop();
    }
    let expected: Vec<String> = expected.iter().map(|l| normalize(l.trim_end())).collect();
    let lines = view_lines(view);
    let keys: Vec<String> = lines.iter().map(|l| normalize(l.text.trim_end())).collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    if expected.is_empty() {
        return out;
    }
    for index in 0..lines.len() {
        if index + expected.len() > lines.len() {
            break;
        }
        if !expected
            .iter()
            .enumerate()
            .all(|(offset, want)| keys[index + offset] == *want)
        {
            continue;
        }
        let last = &lines[index + expected.len() - 1];
        if trailing_newline && !last.newline {
            continue;
        }
        let candidate = (
            lines[index].start,
            if trailing_newline {
                last.end
            } else {
                last.content_end
            },
        );
        if out
            .iter()
            .any(|&(start, end)| end > candidate.0 && start < candidate.1)
        {
            continue;
        }
        out.push(candidate);
    }
    out
}

/// Join the replacement's lines with endings taken from the matched region.
fn with_region_endings(region: &str, replacement: &str, fallback: &str) -> String {
    let mut before: Vec<&str> = Vec::new();
    let mut endings: Vec<&str> = Vec::new();
    let mut rest = region;
    while let Some(pos) = rest.find('\n') {
        let (line, ending) = if pos > 0 && rest.as_bytes()[pos - 1] == b'\r' {
            (&rest[..pos - 1], CRLF)
        } else {
            (&rest[..pos], "\n")
        };
        before.push(line);
        endings.push(ending);
        rest = &rest[pos + 1..];
    }
    before.push(rest);
    let after: Vec<&str> = replacement.split('\n').collect();
    let key = |line: &str| normalize(line.trim_end());
    let before_keys: Vec<String> = before.iter().map(|l| key(l)).collect();
    let after_keys: Vec<String> = after.iter().map(|l| key(l)).collect();
    let source = align_lines(&before_keys, &after_keys);
    let mut out = String::with_capacity(replacement.len() + after.len());
    for (index, line) in after.iter().enumerate() {
        out.push_str(line);
        if index + 1 < after.len() {
            let ending = source[index]
                .and_then(|i| endings.get(i).copied())
                .unwrap_or(fallback);
            out.push_str(ending);
        }
    }
    out
}

/// For each `after` line, the `before` line it keeps (LCS) or, between kept
/// lines, the line it takes the place of.
fn align_lines(before: &[String], after: &[String]) -> Vec<Option<usize>> {
    let (n, m) = (before.len(), after.len());
    if (n + 1) * (m + 1) > MAX_ALIGN_CELLS {
        return (0..m).map(|j| (j < n).then_some(j)).collect();
    }
    let width = m + 1;
    let mut table = vec![0_u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * width + j] = if before[i] == after[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let mut result: Vec<Option<usize>> = vec![None; m];
    let mut gap_before: Vec<usize> = Vec::new();
    let mut gap_after: Vec<usize> = Vec::new();
    let flush = |gb: &mut Vec<usize>, ga: &mut Vec<usize>, result: &mut Vec<Option<usize>>| {
        for (k, &j) in ga.iter().enumerate() {
            result[j] = gb.get(k).copied();
        }
        gb.clear();
        ga.clear();
    };
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if before[i] == after[j] {
            flush(&mut gap_before, &mut gap_after, &mut result);
            result[j] = Some(i);
            i += 1;
            j += 1;
            continue;
        }
        if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            gap_before.push(i);
            i += 1;
        } else {
            gap_after.push(j);
            j += 1;
        }
    }
    gap_before.extend(i..n);
    gap_after.extend(j..m);
    flush(&mut gap_before, &mut gap_after, &mut result);
    result
}

/// Number of sorted positions strictly before `offset`.
fn count_before(positions: &[usize], offset: usize) -> usize {
    positions.partition_point(|&p| p < offset)
}

/// A hint for an `old_string` miss: either `new_string` is already present
/// ("may already be applied"), or the closest numbered lines. Lines vote for
/// where `old_string` starts when they match ignoring all whitespace; at
/// least half must agree.
#[must_use]
pub fn miss_hint(source: &str, old: &str, new: &str) -> Option<String> {
    let view = source.replace(CRLF, "\n");
    let replacement = new.replace(CRLF, "\n");
    if replacement.trim().chars().count() >= 20
        && normalize(&view).contains(&normalize(&replacement))
    {
        return Some(
            "new_string is already in the file, so this edit may already be applied.".into(),
        );
    }
    let key = |line: &str| {
        normalize(line)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let distinct = |line: &str| line.chars().any(char::is_alphanumeric);
    let lines: Vec<&str> = view.split('\n').collect();
    let search_text = old.replace(CRLF, "\n");
    let search: Vec<&str> = search_text.split('\n').collect();
    let mut positions: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let value = key(line);
        if distinct(&value) {
            positions.entry(value).or_default().push(index);
        }
    }
    // Insertion-ordered votes so ties resolve like upstream (first wins).
    let mut order: Vec<isize> = Vec::new();
    let mut votes: HashMap<isize, usize> = HashMap::new();
    let mut wanted = 0;
    for (offset, line) in search.iter().enumerate() {
        let value = key(line);
        if !distinct(&value) {
            continue;
        }
        wanted += 1;
        for &index in positions.get(&value).map_or(&[][..], Vec::as_slice) {
            #[allow(clippy::cast_possible_wrap)]
            let start = index as isize - offset as isize;
            let entry = votes.entry(start).or_insert_with(|| {
                order.push(start);
                0
            });
            *entry += 1;
        }
    }
    let (start, count) = order
        .iter()
        .map(|s| (*s, votes[s]))
        .fold(
            (0, 0),
            |best, vote| if vote.1 > best.1 { vote } else { best },
        );
    if count == 0 || count * 2 < wanted {
        return None;
    }
    let matched: Vec<usize> = search
        .iter()
        .enumerate()
        .filter_map(|(offset, line)| {
            let value = key(line);
            #[allow(clippy::cast_possible_wrap)]
            let index = start + offset as isize;
            let index = usize::try_from(index).ok()?;
            (!value.is_empty() && index < lines.len() && key(lines[index]) == value)
                .then_some(index)
        })
        .collect();
    let from = *matched.first()?;
    let to = (matched.last()? + 1).min(from + HINT_LINES);
    let mut out = vec!["The closest lines in the file:".to_string()];
    for (index, line) in lines[from..to].iter().enumerate() {
        let shown = if line.chars().count() > HINT_LINE_LENGTH {
            format!(
                "{}…",
                line.chars().take(HINT_LINE_LENGTH).collect::<String>()
            )
        } else {
            (*line).to_string()
        };
        out.push(format!("{}: {shown}", from + index + 1));
    }
    Some(out.join("\n"))
}

/// Result of [`apply_unified_patch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patched {
    pub text: String,
    pub hunks: usize,
    /// True when a hunk needed the tiered matcher (moved context, CRLF,
    /// quote or whitespace differences) instead of applying in place.
    pub fuzzy: bool,
}

/// Apply a unified diff to `source` (BOM already removed).
///
/// Hunks apply in place when they match exactly; otherwise each hunk's old
/// block is located with the tiered matcher (the match nearest the hunk's
/// line number wins) and replaced with endings preserved.
///
/// # Errors
/// A message naming the hunk that failed, with a "may already be applied" or
/// closest-lines hint when one is available.
pub fn apply_unified_patch(source: &str, patch: &str) -> Result<Patched, String> {
    let patch = patch.replace(CRLF, "\n");
    let parsed =
        diffy::Patch::from_str(&patch).map_err(|error| format!("invalid patch: {error}"))?;
    let hunks = parsed.hunks();
    if hunks.is_empty() {
        return Err("invalid patch: no @@ hunks found".into());
    }
    if !source.contains('\r') {
        if let Ok(text) = diffy::apply(source, &parsed) {
            return Ok(Patched {
                text,
                hunks: hunks.len(),
                fuzzy: false,
            });
        }
    }
    let ending = line_ending(source, "\n");
    let mut text = source.to_string();
    let mut shift: isize = 0;
    for (number, hunk) in hunks.iter().enumerate() {
        let mut old = String::new();
        let mut new = String::new();
        let (mut old_lines, mut new_lines) = (0_isize, 0_isize);
        for line in hunk.lines() {
            match line {
                diffy::Line::Context(l) => {
                    old.push_str(l);
                    new.push_str(l);
                    old_lines += 1;
                    new_lines += 1;
                }
                diffy::Line::Delete(l) => {
                    old.push_str(l);
                    old_lines += 1;
                }
                diffy::Line::Insert(l) => {
                    new.push_str(l);
                    new_lines += 1;
                }
            }
        }
        #[allow(clippy::cast_possible_wrap)]
        let start = hunk.old_range().start() as isize + shift;
        if old.is_empty() {
            text = insert_at_line(&text, start, &new.replace('\n', ending), ending);
        } else {
            let found = find(&text, &old, &new);
            let Some(best) = found.matches.iter().min_by_key(|m| {
                #[allow(clippy::cast_possible_wrap)]
                let line = line_of(&text, m.start) as isize;
                (line - start).abs()
            }) else {
                let hint = miss_hint(&text, &old, &new)
                    .map_or_else(String::new, |hint| format!("\n\n{hint}"));
                return Err(format!(
                    "hunk {} (@@ -{},{} @@) does not match the file{hint}",
                    number + 1,
                    hunk.old_range().start(),
                    hunk.old_range().len()
                ));
            };
            let best = *best;
            text = replace_matches(&text, &[best], |region| found.render(region, &new));
        }
        shift += new_lines - old_lines;
    }
    Ok(Patched {
        text,
        hunks: hunks.len(),
        fuzzy: true,
    })
}

/// Insert `block` before 1-based `line` (`0` = top of file); past the end
/// appends, adding a final newline first when the file lacks one.
fn insert_at_line(text: &str, line: isize, block: &str, ending: &str) -> String {
    let mut offset = 0;
    let mut remaining = line.max(0);
    while remaining > 0 {
        if let Some(pos) = text[offset..].find('\n') {
            offset += pos + 1;
        } else {
            offset = text.len();
            break;
        }
        remaining -= 1;
    }
    let mut out = String::with_capacity(text.len() + block.len() + 2);
    out.push_str(&text[..offset]);
    if offset == text.len() && !text.is_empty() && !text.ends_with('\n') {
        out.push_str(ending);
    }
    out.push_str(block);
    out.push_str(&text[offset..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_applies_in_place_when_exact() {
        let source = "one\ntwo\nthree\n";
        let patch = "--- a/a\n+++ b/a\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let out = apply_unified_patch(source, patch).expect("patch");
        assert_eq!(out.text, "one\nTWO\nthree\n");
        assert!(!out.fuzzy);
    }

    #[test]
    fn patch_preserves_crlf_and_tolerates_drift() {
        let source = "header\r\none\r\ntwo\r\nthree\r\n";
        // Line numbers are off by one and the patch itself uses LF.
        let patch = "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let out = apply_unified_patch(source, patch).expect("patch");
        assert_eq!(out.text, "header\r\none\r\nTWO\r\nthree\r\n");
        assert!(out.fuzzy);
    }

    #[test]
    fn patch_tolerates_smart_quotes_and_trailing_spaces() {
        let source = "let s = \u{201C}x\u{201D};   \nend\n";
        let patch = "@@ -1,2 +1,2 @@\n-let s = \"x\";\n+let s = \"y\";\n end\n";
        let out = apply_unified_patch(source, patch).expect("patch");
        assert_eq!(out.text, "let s = \"y\";\nend\n");
    }

    #[test]
    fn patch_picks_the_match_nearest_the_hunk_line() {
        let source = "x\nsame\nx\nfiller\nfiller\nx\nsame\nx\n";
        let patch = "@@ -6,3 +6,3 @@\n x\n-same\n+CHANGED\n x\n";
        // Force the fuzzy path by giving the file CRLF-free but wrong offsets.
        let out = apply_unified_patch(source, patch).expect("patch");
        assert_eq!(out.text, "x\nsame\nx\nfiller\nfiller\nx\nCHANGED\nx\n");
    }

    #[test]
    fn patch_reports_already_applied() {
        let source = "fn main() {\n    println!(\"hello, wonderful world\");\n}\n";
        let patch = "@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"hi\");\n+    println!(\"hello, wonderful world\");\n }\n";
        let err = apply_unified_patch(source, patch).expect_err("applied");
        assert!(err.contains("hunk 1"), "{err}");
        assert!(err.contains("may already be applied"), "{err}");
    }

    #[test]
    fn patch_inserts_into_new_and_crlf_files() {
        let created = apply_unified_patch(
            "",
            "--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+hello\n+world\n",
        )
        .expect("create");
        assert_eq!(created.text, "hello\nworld\n");
        let appended = apply_unified_patch("a\r\nb\r\n", "@@ -2,0 +3,1 @@\n+c\n").expect("append");
        assert_eq!(appended.text, "a\r\nb\r\nc\r\n");
        assert!(apply_unified_patch("a", "garbage").is_err());
    }

    fn edit(source: &str, old: &str, new: &str) -> Result<Edited, EditError> {
        apply_edit(source, old, new, false)
    }

    #[test]
    fn exact_match_replaces_once() {
        let out = edit("let a = 1;\nlet b = 2;\n", "let b = 2;", "let b = 3;").expect("edit");
        assert_eq!(out.text, "let a = 1;\nlet b = 3;\n");
        assert_eq!(out.tier, Tier::Exact);
        assert_eq!(out.replacements, 1);
    }

    #[test]
    fn crlf_file_matches_lf_search_and_keeps_crlf() {
        let source = "one\r\ntwo\r\nthree\r\n";
        let out = edit(source, "two\nthree", "TWO\nTHREE\nFOUR").expect("edit");
        assert_eq!(out.text, "one\r\nTWO\r\nTHREE\r\nFOUR\r\n");
        assert_eq!(out.tier, Tier::Exact);
    }

    #[test]
    fn mixed_endings_keep_untouched_lines_bytes() {
        let source = "a\r\nb\nc\r\nd\n";
        let out = edit(source, "b\nc\nd", "b\nC\nd").expect("edit");
        assert_eq!(out.text, "a\r\nb\nC\r\nd\n");
    }

    #[test]
    fn smart_quotes_and_dashes_match_ascii() {
        let source = "say(\u{201C}hello\u{201D}); // it\u{2019}s a \u{2014} test\n";
        let out = edit(source, "say(\"hello\"); // it's a - test", "say(\"bye\");").expect("edit");
        assert_eq!(out.tier, Tier::Unicode);
        assert_eq!(out.text, "say(\"bye\");\n");
        let nbsp = "a\u{00A0}=\u{00A0}1\n";
        assert_eq!(edit(nbsp, "a = 1", "a = 2").expect("nbsp").text, "a = 2\n");
    }

    #[test]
    fn trailing_whitespace_tier_is_line_based() {
        let source = "fn x() {   \n    body();\t\n}\n";
        let out = edit(source, "fn x() {\n    body();\n}", "fn y() {}").expect("edit");
        assert_eq!(out.tier, Tier::TrailingWhitespace);
        assert_eq!(out.text, "fn y() {}\n");
    }

    #[test]
    fn multiple_matches_fail_without_replace_all() {
        let source = "x = 1\ny = 2\nx = 1\n";
        assert_eq!(
            edit(source, "x = 1", "x = 9"),
            Err(EditError::Multiple { count: 2 })
        );
        let all = apply_edit(source, "x = 1", "x = 9", true).expect("all");
        assert_eq!(all.text, "x = 9\ny = 2\nx = 9\n");
        assert_eq!(all.replacements, 2);
    }

    #[test]
    fn already_applied_hint() {
        let source = "fn main() {\n    println!(\"hello, world\");\n}\n";
        let err =
            edit(source, "println!(\"hi\");", "println!(\"hello, world\");").expect_err("miss");
        assert!(err.to_string().contains("may already be applied"), "{err}");
    }

    #[test]
    fn closest_lines_hint_is_numbered() {
        let source = "alpha();\nbeta(1);\ngamma(2);\ndelta();\n";
        let err = edit(source, "beta(1);\ngamma(3);", "x();").expect_err("miss");
        let text = err.to_string();
        assert!(text.contains("The closest lines in the file:"), "{text}");
        assert!(text.contains("2: beta(1);"), "{text}");
        assert!(!text.contains("4: delta"), "{text}");
    }

    #[test]
    fn no_hint_when_nothing_is_close() {
        let err = edit("abc\n", "zzz qqq", "y").expect_err("miss");
        assert_eq!(err, EditError::NotFound { hint: None });
    }

    #[test]
    fn rejects_empty_identical_and_noop() {
        assert_eq!(edit("a", "", "b"), Err(EditError::EmptyOld));
        assert_eq!(edit("a", "a", "a"), Err(EditError::Identical));
        // Tier 3 matches `x` for `x  `, and replacing it with `x` changes nothing.
        assert_eq!(
            edit("x\n", "x  ", "x"),
            Err(EditError::Unchanged {
                endings_only: false
            })
        );
    }

    #[test]
    fn line_ending_only_edit_applies_to_bytes() {
        let out = edit("a\nb\n", "a\nb\n", "a\r\nb\r\n").expect("edit");
        assert_eq!(out.text, "a\r\nb\r\n");
        assert_eq!(out.tier, Tier::LineEndings);
        assert_eq!(edit("a\r\n", "a\r\n", "a\r\n"), Err(EditError::Identical));
    }

    #[test]
    fn bom_round_trip() {
        let (bom, body) = split_bom("\u{feff}hello\n");
        assert!(bom);
        assert_eq!(body, "hello\n");
        let edited = edit(body, "hello", "bye").expect("edit");
        assert_eq!(join_bom(bom, &edited.text), "\u{feff}bye\n");
        assert_eq!(join_bom(true, "\u{feff}x"), "\u{feff}x");
        assert_eq!(join_bom(false, "x"), "x");
    }

    #[test]
    fn unicode_offsets_map_back_to_bytes() {
        let source = "é \u{201C}q\u{201D} ü\r\nnext\r\n";
        let out = edit(source, "\"q\" ü\nnext", "\"r\" ü\nnext").expect("edit");
        assert_eq!(out.text, "é \"r\" ü\r\nnext\r\n");
    }

    #[test]
    fn line_of_counts_newlines() {
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 4), 3);
    }
}
