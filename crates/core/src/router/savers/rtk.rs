// Design adapted from 9Router (MIT, © 2024-2026 decolua and contributors): open-sse/rtk/index.js, autodetect.js, constants.js, filters/* @ 1855aa4
//! RTK: compress tool results before they are sent back to the model.
//!
//! Detection order, thresholds and limits follow 9Router; the compact output
//! formats are our own. A result is skipped when it is an error, shorter than
//! [`MIN_CHARS`] or longer than [`MAX_BYTES`], and the compressed text is kept
//! only when it is non-empty and strictly shorter than the original.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

pub const MIN_CHARS: usize = 500;
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Detectors only look at this many leading characters.
pub const HEAD_CHARS: usize = 1024;

const DIFF_HUNK_LINES: usize = 100;
const DIFF_CONTEXT: usize = 3;
const GIT_LOG_LINES: usize = 200;
const GREP_PER_FILE: usize = 10;
const FIND_PER_DIR: usize = 10;
const FIND_DIRS: usize = 20;
const STATUS_LIMIT: usize = 10;
const LS_TOP_EXTENSIONS: usize = 5;
const LS_NOISE: &[&str] = &["node_modules", ".git", "target", "dist"];
const TREE_LINES: usize = 200;
const TRUNCATE_HEAD: usize = 120;
const TRUNCATE_TAIL: usize = 60;
const LONG_OUTPUT_LINES: usize = 250;

/// Which filter compressed a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Filter {
    GitLog,
    GitDiff,
    GitStatus,
    Build,
    Porcelain,
    Grep,
    Find,
    Tree,
    Ls,
    SearchList,
    ReadNumbered,
    DedupLog,
    SmartTruncate,
}

impl Filter {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitLog => "git-log",
            Self::GitDiff => "git-diff",
            Self::GitStatus => "git-status",
            Self::Build => "build",
            Self::Porcelain => "porcelain",
            Self::Grep => "grep",
            Self::Find => "find",
            Self::Tree => "tree",
            Self::Ls => "ls",
            Self::SearchList => "search-list",
            Self::ReadNumbered => "read-numbered",
            Self::DedupLog => "dedup-log",
            Self::SmartTruncate => "smart-truncate",
        }
    }
}

/// Compress one tool result. `None` = leave it unchanged.
#[must_use]
pub fn compress(text: &str) -> Option<(String, Filter)> {
    if text.chars().count() < MIN_CHARS || text.len() > MAX_BYTES {
        return None;
    }
    let filter = detect(text)?;
    let out = run(filter, text);
    (!out.trim().is_empty() && out.len() < text.len()).then_some((out, filter))
}

fn head(text: &str) -> &str {
    match text.char_indices().nth(HEAD_CHARS) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

fn ratio(lines: &[&str], pred: impl Fn(&str) -> bool) -> (usize, f64) {
    let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.is_empty() {
        return (0, 0.0);
    }
    let hits = non_empty.iter().filter(|l| pred(l)).count();
    #[allow(clippy::cast_precision_loss)]
    let share = hits as f64 / non_empty.len() as f64;
    (hits, share)
}

/// True when at least `min_hits` non-empty lines match and they make up `min_share` of them.
fn dominant(lines: &[&str], pred: impl Fn(&str) -> bool, min_hits: usize, min_share: f64) -> bool {
    let (hits, share) = ratio(lines, pred);
    hits >= min_hits && share >= min_share
}

/// First matching detector, in 9Router's order.
#[must_use]
pub fn detect(text: &str) -> Option<Filter> {
    let head = head(text);
    // The last head line may be cut mid-way; ignore it for line shapes.
    let mut head_lines: Vec<&str> = head.lines().collect();
    if head.len() < text.len() && head_lines.len() > 1 {
        head_lines.pop();
    }
    let all_lines: Vec<&str> = text.lines().collect();

    if head_lines.iter().any(|l| is_commit_line(l)) {
        return Some(Filter::GitLog);
    }
    if head_lines
        .iter()
        .any(|l| l.starts_with("diff --git ") || l.starts_with("@@ -"))
        || head_lines
            .windows(2)
            .any(|w| w[0].starts_with("--- ") && w[1].starts_with("+++ "))
    {
        return Some(Filter::GitDiff);
    }
    if head_lines.iter().any(|l| {
        l.starts_with("On branch ")
            || l.starts_with("HEAD detached")
            || matches!(
                l.trim(),
                "Changes to be committed:" | "Changes not staged for commit:" | "Untracked files:"
            )
    }) {
        return Some(Filter::GitStatus);
    }
    if head_lines.iter().filter(|l| is_build_line(l)).count() >= 2 {
        return Some(Filter::Build);
    }
    if dominant(&head_lines, is_porcelain_line, 3, 0.6) {
        return Some(Filter::Porcelain);
    }
    if dominant(&head_lines, |l| grep_parts(l).is_some(), 3, 0.6) {
        return Some(Filter::Grep);
    }
    if dominant(&head_lines, is_path_line, 5, 0.8) {
        return Some(Filter::Find);
    }
    if head.contains("├── ") || head.contains("└── ") {
        return Some(Filter::Tree);
    }
    let first = head_lines.first().map_or("", |l| l.trim());
    let perms = head_lines.iter().filter(|l| is_perm_line(l)).count();
    if first.strip_prefix("total ").is_some_and(|n| {
        n.trim()
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c.is_ascii_alphabetic())
    }) || perms >= 3
    {
        return Some(Filter::Ls);
    }
    if dominant(&head_lines, |l| count_parts(l).is_some(), 3, 0.6) {
        return Some(Filter::SearchList);
    }
    if all_lines.len() >= LONG_OUTPUT_LINES {
        let (_, share) = ratio(&all_lines, is_numbered_line);
        if share >= 0.7 {
            return Some(Filter::ReadNumbered);
        }
    }
    let non_empty = all_lines.iter().filter(|l| !l.trim().is_empty()).count();
    if non_empty >= 5 && duplicate_share(&all_lines) >= 0.2 {
        return Some(Filter::DedupLog);
    }
    if all_lines.len() >= LONG_OUTPUT_LINES {
        return Some(Filter::SmartTruncate);
    }
    None
}

fn run(filter: Filter, text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    match filter {
        Filter::GitLog => git_log(&lines),
        Filter::GitDiff => git_diff(&lines),
        Filter::GitStatus => git_status(&lines),
        Filter::Build => build(&lines),
        Filter::Porcelain => porcelain(&lines),
        Filter::Grep => grep(&lines),
        Filter::Find => find(&lines),
        Filter::Tree => tree(&lines),
        Filter::Ls => ls(&lines),
        Filter::SearchList => search_list(&lines),
        Filter::ReadNumbered => read_numbered(&lines),
        Filter::DedupLog => {
            let deduped = dedup_log(&lines);
            let out_lines: Vec<&str> = deduped.lines().collect();
            if out_lines.len() >= LONG_OUTPUT_LINES {
                smart_truncate(&out_lines)
            } else {
                deduped
            }
        }
        Filter::SmartTruncate => smart_truncate(&lines),
    }
}

// ---- line shapes --------------------------------------------------------------

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_commit_line(line: &str) -> bool {
    line.strip_prefix("commit ").is_some_and(|rest| {
        let sha = rest.split_whitespace().next().unwrap_or("");
        (7..=40).contains(&sha.len()) && is_hex(sha)
    })
}

fn is_build_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("Compiling ")
        || t.starts_with("Checking ")
        || t.starts_with("Finished ")
        || t.starts_with("error[E")
        || t.starts_with("warning: ")
        || t.starts_with("error: ")
        || t.starts_with("npm ERR!")
        || t.starts_with("npm warn")
        || (t.starts_with("error TS") || t.contains(": error TS"))
        || t.starts_with("> tsc")
        || t.starts_with("BUILD ")
        || t.starts_with("[ERROR]")
        || t.starts_with("[WARNING]")
        || t.starts_with("webpack compiled")
        || t.starts_with("vite v")
}

fn is_porcelain_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    let code = |b: u8| b" MADRCU?!T".contains(&b);
    bytes.len() > 3
        && code(bytes[0])
        && code(bytes[1])
        && bytes[2] == b' '
        && bytes[3] != b' '
        && !(bytes[0] == b' ' && bytes[1] == b' ')
}

/// `path:line:text` → `(path, line, text)`.
fn grep_parts(line: &str) -> Option<(&str, &str, &str)> {
    let (path, rest) = line.split_once(':')?;
    // Paths never start with a digit here; that excludes timestamps (`12:00:01`).
    if path.is_empty()
        || path.starts_with(|c: char| c.is_whitespace() || c.is_ascii_digit())
        || path.contains("  ")
    {
        return None;
    }
    let (number, text) = rest.split_once(':')?;
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .then_some((path, number, text))
}

/// `path:count` → `(path, count)`.
fn count_parts(line: &str) -> Option<(&str, u64)> {
    let (path, count) = line.trim_end().rsplit_once(':')?;
    if path.is_empty() || path.starts_with(char::is_whitespace) {
        return None;
    }
    count.parse().ok().map(|n| (path, n))
}

fn is_path_line(line: &str) -> bool {
    let t = line.trim_end();
    !t.is_empty()
        && t.len() < 300
        && (t.starts_with("./") || t.starts_with('/') || t.contains('/'))
        && !t.contains(": ")
        && !t.contains('│')
        && !t.starts_with(char::is_whitespace)
        && count_parts(t).is_none()
        && grep_parts(t).is_none()
}

fn is_perm_line(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    "-dlcbsp".contains(first)
        && chars
            .by_ref()
            .take(9)
            .filter(|c| "rwxsStT-".contains(*c))
            .count()
            == 9
}

fn is_numbered_line(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && t[digits..].starts_with(['|', '\t', '→', '│'])
}

/// Comparison key for log lines: leading timestamps dropped, digit runs → `#`.
fn normalize(line: &str) -> String {
    let trimmed = line.trim_start_matches(|c: char| {
        c.is_ascii_digit()
            || matches!(
                c,
                '-' | ':' | '.' | 'T' | 'Z' | '[' | ']' | ' ' | ',' | '+' | '/'
            )
    });
    let mut out = String::with_capacity(trimmed.len());
    let mut in_digits = false;
    for c in trimmed.trim_end().chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

fn duplicate_share(lines: &[&str]) -> f64 {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        *seen.entry(normalize(line)).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let dupes: usize = seen.values().map(|n| n - 1).sum();
    #[allow(clippy::cast_precision_loss)]
    let share = dupes as f64 / total as f64;
    share
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

fn list(items: &[String], limit: usize) -> String {
    let mut out = items
        .iter()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > limit {
        let _ = write!(out, ", … +{} more", items.len() - limit);
    }
    out
}

// ---- filters ------------------------------------------------------------------

/// One line per commit: `sha7 date author: subject`, plus `--stat` lines.
fn git_log(lines: &[&str]) -> String {
    struct Commit {
        sha: String,
        author: String,
        date: String,
        subject: String,
        stats: Vec<String>,
        files: usize,
    }
    let mut commits: Vec<Commit> = Vec::new();
    for line in lines {
        if is_commit_line(line) {
            let sha = line[7..].split_whitespace().next().unwrap_or("");
            commits.push(Commit {
                sha: sha.chars().take(7).collect(),
                author: String::new(),
                date: String::new(),
                subject: String::new(),
                stats: Vec::new(),
                files: 0,
            });
            continue;
        }
        let Some(commit) = commits.last_mut() else {
            continue;
        };
        if let Some(author) = line.strip_prefix("Author:") {
            let name = author.trim();
            commit.author = name.split(" <").next().unwrap_or(name).to_string();
        } else if let Some(date) = line.strip_prefix("Date:") {
            let parts: Vec<&str> = date.split_whitespace().collect();
            commit.date = if parts.len() >= 5 {
                format!("{} {} {}", parts[1], parts[2], parts[4])
            } else {
                date.trim().to_string()
            };
        } else if let Some(message) = line.strip_prefix("    ") {
            if commit.subject.is_empty() && !message.trim().is_empty() {
                commit.subject = message.trim().to_string();
            }
        } else if line.starts_with("diff --git ") {
            commit.files += 1;
        } else if line.starts_with(' ') && (line.contains(" | ") || line.contains("changed")) {
            commit.stats.push(line.trim().to_string());
        }
    }
    let mut out = Vec::new();
    let total = commits.len();
    for commit in commits {
        if out.len() >= GIT_LOG_LINES {
            break;
        }
        let mut line = format!(
            "{} {} {}: {}",
            commit.sha, commit.date, commit.author, commit.subject
        );
        if commit.files > 0 {
            let _ = write!(line, " ({} files changed)", commit.files);
        }
        out.push(line);
        for stat in commit.stats.iter().take(5) {
            out.push(format!("  {stat}"));
        }
        if commit.stats.len() > 5 {
            out.push(format!("  … +{} more stat lines", commit.stats.len() - 5));
        }
    }
    let shown = out.iter().filter(|l| !l.starts_with("  ")).count();
    if shown < total {
        out.push(format!("… {} more commits", total - shown));
    }
    out.join("\n")
}

/// Per file: `== path (+A -D)`, hunks with ≤ 3 context lines around changes,
/// each hunk capped at 100 lines.
#[allow(clippy::too_many_lines)] // A small state machine over diff lines.
fn git_diff(lines: &[&str]) -> String {
    struct FileDiff {
        name: String,
        note: Vec<String>,
        body: Vec<String>,
        added: usize,
        removed: usize,
    }
    fn flush_hunk(hunk: &mut Vec<&str>, file: &mut FileDiff) {
        if hunk.is_empty() {
            return;
        }
        let changed: Vec<bool> = hunk
            .iter()
            .map(|l| l.starts_with('+') || l.starts_with('-') || l.starts_with('\\'))
            .collect();
        let mut keep = vec![false; hunk.len()];
        for (i, is_change) in changed.iter().enumerate() {
            if *is_change {
                let lo = i.saturating_sub(DIFF_CONTEXT);
                let hi = (i + DIFF_CONTEXT).min(hunk.len() - 1);
                keep[lo..=hi].iter_mut().for_each(|k| *k = true);
            }
        }
        let mut emitted = 0usize;
        let mut skipped = 0usize;
        let mut i = 0;
        while i < hunk.len() {
            if emitted >= DIFF_HUNK_LINES {
                file.body
                    .push(format!("… {} more lines in hunk", hunk.len() - i));
                break;
            }
            if keep[i] {
                if skipped > 0 {
                    file.body.push(format!(" ⋯ {skipped} unchanged"));
                    skipped = 0;
                }
                file.body.push(hunk[i].to_string());
                emitted += 1;
            } else {
                skipped += 1;
            }
            i += 1;
        }
        hunk.clear();
    }

    let mut files: Vec<FileDiff> = Vec::new();
    let mut hunk: Vec<&str> = Vec::new();
    let mut in_hunk = false;
    for line in lines {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(file) = files.last_mut() {
                flush_hunk(&mut hunk, file);
            }
            in_hunk = false;
            let name = rest.rsplit(" b/").next().unwrap_or(rest).to_string();
            files.push(FileDiff {
                name,
                note: Vec::new(),
                body: Vec::new(),
                added: 0,
                removed: 0,
            });
            continue;
        }
        if files.is_empty() {
            files.push(FileDiff {
                name: String::new(),
                note: Vec::new(),
                body: Vec::new(),
                added: 0,
                removed: 0,
            });
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("@@") {
            flush_hunk(&mut hunk, file);
            in_hunk = true;
            file.body.push((*line).to_string());
            continue;
        }
        if !in_hunk {
            if let Some(path) = line.strip_prefix("+++ ") {
                if file.name.is_empty() {
                    file.name = path.trim_start_matches("b/").to_string();
                }
            } else if line.starts_with("new file") {
                file.note.push("new".into());
            } else if line.starts_with("deleted file") {
                file.note.push("deleted".into());
            } else if let Some(from) = line.strip_prefix("rename from ") {
                file.note.push(format!("renamed from {from}"));
            } else if line.starts_with("Binary files") {
                file.note.push("binary".into());
            }
            continue;
        }
        if line.starts_with('+') {
            file.added += 1;
        } else if line.starts_with('-') {
            file.removed += 1;
        }
        hunk.push(line);
    }
    if let Some(file) = files.last_mut() {
        flush_hunk(&mut hunk, file);
    }
    let mut out = Vec::new();
    for file in files {
        let note = if file.note.is_empty() {
            String::new()
        } else {
            format!(", {}", file.note.join(", "))
        };
        out.push(format!(
            "== {} (+{} -{}{note})",
            file.name, file.added, file.removed
        ));
        out.extend(file.body);
    }
    out.join("\n")
}

#[derive(Default)]
struct StatusGroups {
    branch: Vec<String>,
    staged: Vec<String>,
    modified: Vec<String>,
    untracked: Vec<String>,
    conflicts: Vec<String>,
    clean: bool,
}

impl StatusGroups {
    fn render(&self) -> String {
        let mut out = self.branch.clone();
        for (label, items) in [
            ("conflicts", &self.conflicts),
            ("staged", &self.staged),
            ("modified", &self.modified),
            ("untracked", &self.untracked),
        ] {
            if !items.is_empty() {
                out.push(format!(
                    "{label} ({}): {}",
                    items.len(),
                    list(items, STATUS_LIMIT)
                ));
            }
        }
        if self.clean {
            out.push("clean".into());
        }
        out.join("\n")
    }
}

fn git_status(lines: &[&str]) -> String {
    let mut groups = StatusGroups::default();
    let mut section = "";
    for line in lines {
        let t = line.trim();
        if let Some(branch) = t.strip_prefix("On branch ") {
            groups.branch.push(format!("branch {branch}"));
        } else if t.starts_with("HEAD detached") || t.starts_with("Your branch") {
            groups.branch.push(t.trim_end_matches('.').to_string());
        } else if t == "Changes to be committed:" {
            section = "staged";
        } else if t == "Changes not staged for commit:" {
            section = "modified";
        } else if t == "Untracked files:" {
            section = "untracked";
        } else if t == "Unmerged paths:" {
            section = "conflicts";
        } else if t.starts_with("nothing to commit") {
            groups.clean = true;
        } else if t.is_empty() || t.starts_with('(') || t.starts_with("no changes added") {
        } else if line.starts_with('\t') || line.starts_with("  ") {
            let entry = match t.split_once(':') {
                Some((kind, path)) if section != "untracked" => {
                    let tag = match kind.trim() {
                        "new file" => "A",
                        "deleted" => "D",
                        "renamed" => "R",
                        "both modified" | "both added" => "U",
                        _ => "M",
                    };
                    format!("{tag} {}", path.trim())
                }
                _ => t.to_string(),
            };
            match section {
                "staged" => groups.staged.push(entry),
                "modified" => groups.modified.push(entry),
                "untracked" => groups.untracked.push(entry),
                "conflicts" => groups.conflicts.push(entry),
                _ => {}
            }
        }
    }
    groups.render()
}

fn porcelain(lines: &[&str]) -> String {
    let mut groups = StatusGroups::default();
    for line in lines {
        if let Some(branch) = line.strip_prefix("## ") {
            groups.branch.push(format!("branch {branch}"));
            continue;
        }
        if !is_porcelain_line(line) {
            continue;
        }
        let (x, y) = (line.as_bytes()[0], line.as_bytes()[1]);
        let path = line[3..].trim().to_string();
        if x == b'?' {
            groups.untracked.push(path);
        } else if x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D') {
            groups.conflicts.push(path);
        } else {
            if x != b' ' {
                groups.staged.push(format!("{} {path}", char::from(x)));
            }
            if y != b' ' {
                groups.modified.push(format!("{} {path}", char::from(y)));
            }
        }
    }
    groups.render()
}

/// Drops progress noise, keeps diagnostics, folds identical warning blocks.
fn build(lines: &[&str]) -> String {
    const NOISE: &[&str] = &[
        "Compiling ",
        "Checking ",
        "Downloading ",
        "Downloaded ",
        "Fresh ",
        "Updating ",
        "Locking ",
        "Adding ",
        "Blocking ",
        "Building [",
        "Running `",
        "Documenting ",
    ];
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut dropped = 0usize;
    for line in lines {
        let t = line.trim_start();
        if NOISE.iter().any(|n| t.starts_with(n)) {
            dropped += 1;
            continue;
        }
        let starts_block = t.starts_with("warning") || t.starts_with("error") || blocks.is_empty();
        if starts_block {
            blocks.push(vec![line]);
        } else if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut order: Vec<(String, Vec<&str>)> = Vec::new();
    for block in blocks {
        let key = block.join("\n");
        let count = seen.entry(key.clone()).or_default();
        *count += 1;
        if *count == 1 {
            order.push((key, block));
        }
    }
    let mut out = Vec::new();
    let mut warnings = 0usize;
    for (key, block) in order {
        let first = block.first().map_or("", |l| l.trim_start());
        let repeat = seen.get(&key).copied().unwrap_or(1);
        let suffix = if repeat > 1 {
            format!(" (×{repeat})")
        } else {
            String::new()
        };
        if first.starts_with("warning") {
            warnings += 1;
            if warnings > 20 {
                out.push(format!("{first}{suffix}"));
                continue;
            }
        }
        for (i, line) in block.iter().enumerate() {
            if i == 0 {
                out.push(format!("{line}{suffix}"));
            } else if !line.trim().is_empty() {
                out.push((*line).to_string());
            }
        }
    }
    if dropped > 0 {
        out.push(format!("({dropped} progress lines dropped)"));
    }
    out.join("\n")
}

/// `path` + up to 10 `  N: text` lines per file.
fn grep(lines: &[&str]) -> String {
    let mut files: Vec<(&str, Vec<String>)> = Vec::new();
    let mut total = 0usize;
    for line in lines {
        let Some((path, number, text)) = grep_parts(line) else {
            continue;
        };
        total += 1;
        let index = if let Some(i) = files.iter().position(|(p, _)| *p == path) {
            i
        } else {
            files.push((path, Vec::new()));
            files.len() - 1
        };
        files[index]
            .1
            .push(format!("  {number}: {}", clip(text.trim(), 200)));
    }
    let mut out = vec![format!("{total} matches in {} files", files.len())];
    for (path, matches) in files {
        out.push(path.to_string());
        out.extend(matches.iter().take(GREP_PER_FILE).cloned());
        if matches.len() > GREP_PER_FILE {
            out.push(format!(
                "  … +{} more in file",
                matches.len() - GREP_PER_FILE
            ));
        }
    }
    out.join("\n")
}

/// Paths grouped by directory: ≤ 10 files per directory, ≤ 20 directories.
fn find(lines: &[&str]) -> String {
    let mut dirs: Vec<(String, Vec<String>)> = Vec::new();
    let mut total = 0usize;
    for line in lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty()) {
        total += 1;
        let (dir, name) = match line.trim_end_matches('/').rsplit_once('/') {
            Some((dir, name)) => (if dir.is_empty() { "/" } else { dir }, name),
            None => (".", line),
        };
        match dirs.iter_mut().find(|(d, _)| d == dir) {
            Some((_, names)) => names.push(name.to_string()),
            None => dirs.push((dir.to_string(), vec![name.to_string()])),
        }
    }
    let mut out = vec![format!("{total} paths in {} directories", dirs.len())];
    for (dir, names) in dirs.iter().take(FIND_DIRS) {
        out.push(format!(
            "{dir}/ ({}): {}",
            names.len(),
            list(names, FIND_PER_DIR)
        ));
    }
    if dirs.len() > FIND_DIRS {
        let rest: usize = dirs[FIND_DIRS..].iter().map(|(_, n)| n.len()).sum();
        out.push(format!(
            "… +{} more directories ({rest} paths)",
            dirs.len() - FIND_DIRS
        ));
    }
    out.join("\n")
}

/// Drops the contents of noise directories; caps at 200 lines; keeps the
/// summary line.
fn tree(lines: &[&str]) -> String {
    let depth = |line: &str| {
        line.find(|c: char| !"│├└─ \u{a0}".contains(c))
            .unwrap_or(line.len())
    };
    let mut out: Vec<String> = Vec::new();
    let mut skip_deeper: Option<usize> = None;
    let mut hidden = 0usize;
    let summary = lines
        .last()
        .filter(|l| l.contains("director") && l.contains("file"))
        .map(|l| (*l).to_string());
    let body = if summary.is_some() {
        &lines[..lines.len() - 1]
    } else {
        lines
    };
    for line in body {
        let d = depth(line);
        if let Some(limit) = skip_deeper {
            if d > limit {
                hidden += 1;
                continue;
            }
            skip_deeper = None;
        }
        let name = line[d..].trim().trim_end_matches('/');
        if LS_NOISE.contains(&name) {
            out.push(format!("{} (contents omitted)", line.trim_end()));
            skip_deeper = Some(d);
            continue;
        }
        out.push((*line).to_string());
    }
    let mut kept: Vec<String> = out.iter().take(TREE_LINES).cloned().collect();
    if out.len() > TREE_LINES {
        kept.push(format!("… +{} more entries", out.len() - TREE_LINES));
    }
    if hidden > 0 {
        kept.push(format!("({hidden} entries inside omitted directories)"));
    }
    if let Some(summary) = summary {
        kept.push(summary);
    }
    kept.join("\n")
}

/// Directories, files (first 30) and the top 5 extensions.
fn ls(lines: &[&str]) -> String {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut omitted = Vec::new();
    let mut extensions: BTreeMap<String, usize> = BTreeMap::new();
    for line in lines {
        let t = line.trim();
        if t.is_empty() || t.starts_with("total ") {
            continue;
        }
        let (is_dir, name) = if is_perm_line(t) {
            let fields: Vec<&str> = t.split_whitespace().collect();
            if fields.len() < 9 {
                continue;
            }
            let name = fields[8..].join(" ");
            let name = name.split(" -> ").next().unwrap_or(&name).to_string();
            (t.starts_with('d'), name)
        } else {
            (t.ends_with('/'), t.trim_end_matches('/').to_string())
        };
        if name == "." || name == ".." {
            continue;
        }
        if LS_NOISE.contains(&name.as_str()) {
            omitted.push(format!("{name}/"));
            continue;
        }
        if is_dir {
            dirs.push(format!("{name}/"));
        } else {
            let ext = match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => format!(".{ext}"),
                _ => "(none)".into(),
            };
            *extensions.entry(ext).or_default() += 1;
            files.push(name);
        }
    }
    let mut out = Vec::new();
    if !dirs.is_empty() {
        out.push(format!("dirs ({}): {}", dirs.len(), list(&dirs, 30)));
    }
    if !files.is_empty() {
        out.push(format!("files ({}): {}", files.len(), list(&files, 30)));
        let mut top: Vec<(String, usize)> = extensions.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let top: Vec<String> = top
            .into_iter()
            .take(LS_TOP_EXTENSIONS)
            .map(|(ext, n)| format!("{ext} {n}"))
            .collect();
        out.push(format!("by extension: {}", top.join(", ")));
    }
    if !omitted.is_empty() {
        out.push(format!("omitted: {}", omitted.join(", ")));
    }
    out.join("\n")
}

/// `path:count` lists: sorted by count, top 30.
fn search_list(lines: &[&str]) -> String {
    let mut entries: Vec<(&str, u64)> = lines.iter().filter_map(|l| count_parts(l)).collect();
    let total: u64 = entries.iter().map(|(_, n)| n).sum();
    let files = entries.len();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let mut out = vec![format!("{total} matches in {files} files")];
    out.extend(
        entries
            .iter()
            .take(30)
            .map(|(path, n)| format!("{path} ({n})")),
    );
    if files > 30 {
        out.push(format!("… +{} more files", files - 30));
    }
    out.join("\n")
}

fn read_numbered(lines: &[&str]) -> String {
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
    let total = trimmed.len();
    let mut out = vec![format!(
        "(numbered file excerpt: {total} lines, showing the first {TRUNCATE_HEAD} and last {TRUNCATE_TAIL})"
    )];
    out.push(smart_truncate(&trimmed));
    out.join("\n")
}

/// Collapse consecutive repeats (`(×N)`) and drop lines seen more than 3
/// times overall.
fn dedup_log(lines: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut dropped = 0usize;
    let mut i = 0;
    while i < lines.len() {
        let key = normalize(lines[i]);
        let mut run = 1;
        while i + run < lines.len() && !key.is_empty() && normalize(lines[i + run]) == key {
            run += 1;
        }
        let count = seen.entry(key.clone()).or_default();
        *count += 1;
        if *count > 3 && !key.is_empty() {
            dropped += run;
        } else if run > 1 {
            out.push(format!("{} (×{run})", lines[i].trim_end()));
        } else {
            out.push(lines[i].trim_end().to_string());
        }
        i += run;
    }
    if dropped > 0 {
        out.push(format!("({dropped} repeated lines dropped)"));
    }
    out.join("\n")
}

fn smart_truncate<S: AsRef<str>>(lines: &[S]) -> String {
    if lines.len() <= TRUNCATE_HEAD + TRUNCATE_TAIL {
        return lines
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut out: Vec<&str> = lines[..TRUNCATE_HEAD].iter().map(AsRef::as_ref).collect();
    let omitted = format!(
        "… {} lines omitted …",
        lines.len() - TRUNCATE_HEAD - TRUNCATE_TAIL
    );
    out.push(&omitted);
    out.extend(
        lines[lines.len() - TRUNCATE_TAIL..]
            .iter()
            .map(AsRef::as_ref),
    );
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_git_log() -> String {
        let mut s = String::new();
        for i in 0..12 {
            let _ = write!(
                s,
                "commit {i:x}abcdef0123456789abcdef0123456789abcde\nAuthor: Dev Person <dev@example.invalid>\nDate:   Mon Nov 10 12:00:0{} 2025 +0100\n\n    feat: change number {i}\n\n    Longer body text that explains the change in detail.\n\n",
                i % 10
            );
        }
        s
    }

    #[test]
    fn git_log_one_line_per_commit() {
        let text = fixture_git_log();
        assert_eq!(detect(&text), Some(Filter::GitLog));
        let (out, filter) = compress(&text).expect("compressed");
        assert_eq!(filter, Filter::GitLog);
        assert_eq!(out.lines().count(), 12);
        assert!(
            out.starts_with("0abcdef Nov 10 2025 Dev Person: feat: change number 0"),
            "{out}"
        );
        assert!(!out.contains("Longer body"));
    }

    #[test]
    fn git_diff_trims_context_and_counts() {
        let mut text = String::from("diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,30 +1,30 @@\n");
        for i in 0..12 {
            let _ = writeln!(text, " context line {i}");
        }
        text.push_str("-old line\n+new line\n");
        for i in 0..12 {
            let _ = writeln!(text, " trailing context {i}");
        }
        text.push_str("diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n");
        assert_eq!(detect(&text), Some(Filter::GitDiff));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("== src/lib.rs (+1 -1)"), "{out}");
        assert!(out.contains(" ⋯ 9 unchanged"), "{out}");
        assert!(
            out.contains(" context line 9") && !out.contains(" context line 8"),
            "{out}"
        );
        assert!(out.contains(" trailing context 2") && !out.contains(" trailing context 3"));
        assert!(out.contains("== new.txt (+1 -0, new)"), "{out}");
    }

    #[test]
    fn git_diff_caps_hunks() {
        let mut text = String::from("diff --git a/a b/a\n@@ -1,300 +1,300 @@\n");
        for i in 0..300 {
            let _ = writeln!(text, "+added {i}");
        }
        let out = git_diff(&text.lines().collect::<Vec<_>>());
        assert!(out.contains("… 200 more lines in hunk"), "{out}");
    }

    #[test]
    fn git_status_groups_and_limits() {
        let mut text = String::from("On branch main\nYour branch is ahead of 'origin/main' by 2 commits.\n  (use \"git push\" to publish your local commits)\n\nChanges to be committed:\n  (use \"git restore --staged <file>...\" to unstage)\n\tnew file:   added.rs\n\nChanges not staged for commit:\n");
        for i in 0..14 {
            let _ = writeln!(text, "\tmodified:   src/module_{i}.rs");
        }
        text.push_str("\nUntracked files:\n  (use \"git add <file>...\" to include in what will be committed)\n\tscratch.txt\n");
        assert_eq!(detect(&text), Some(Filter::GitStatus));
        let (out, _) = compress(&text).expect("compressed");
        assert!(
            out.starts_with("branch main\nYour branch is ahead of 'origin/main' by 2 commits"),
            "{out}"
        );
        assert!(out.contains("staged (1): A added.rs"));
        assert!(out.contains("modified (14): M src/module_0.rs"));
        assert!(out.contains("… +4 more"));
        assert!(out.contains("untracked (1): scratch.txt"));
    }

    #[test]
    fn build_output_drops_progress_and_folds_repeats() {
        let mut text = String::new();
        for i in 0..20 {
            let _ = writeln!(text, "   Compiling crate-{i} v0.1.{i}");
        }
        for _ in 0..3 {
            text.push_str("warning: unused variable: `x`\n --> src/main.rs:2:9\n  |\n2 |     let x = 1;\n  |         ^\n\n");
        }
        text.push_str("error[E0425]: cannot find value `y` in this scope\n --> src/main.rs:3:5\n\nerror: could not compile `demo`\n");
        assert_eq!(detect(&text), Some(Filter::Build));
        let (out, _) = compress(&text).expect("compressed");
        assert!(!out.contains("Compiling"));
        assert!(out.contains("warning: unused variable: `x` (×3)"), "{out}");
        assert!(out.contains("error[E0425]"));
        assert!(out.contains("(20 progress lines dropped)"));
    }

    #[test]
    fn porcelain_status_is_grouped() {
        let mut text = String::from("## main...origin/main\n");
        for i in 0..15 {
            let _ = writeln!(text, " M src/changed_file_number_{i}.rs");
        }
        text.push_str("A  staged.rs\n?? new_untracked_file.txt\nUU conflicted.rs\n");
        assert_eq!(detect(&text), Some(Filter::Porcelain));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("branch main...origin/main"));
        assert!(out.contains("conflicts (1): conflicted.rs"));
        assert!(out.contains("staged (1): A staged.rs"));
        assert!(out.contains("modified (15)"));
        assert!(out.contains("untracked (1): new_untracked_file.txt"));
    }

    #[test]
    fn grep_groups_by_file_with_limit() {
        let mut text = String::new();
        for i in 0..14 {
            let _ = writeln!(
                text,
                "src/router/client.rs:{}:    let value = compute_something({i});",
                i + 1
            );
        }
        text.push_str("src/lib.rs:3:pub mod router;\n");
        assert_eq!(detect(&text), Some(Filter::Grep));
        let (out, _) = compress(&text).expect("compressed");
        assert!(
            out.starts_with("15 matches in 2 files\nsrc/router/client.rs\n  1: let value"),
            "{out}"
        );
        assert!(out.contains("  … +4 more in file"));
        assert!(out.contains("src/lib.rs\n  3: pub mod router;"));
    }

    #[test]
    fn find_groups_by_directory() {
        let mut text = String::new();
        for d in 0..25 {
            for f in 0..3 {
                let _ = writeln!(text, "./src/dir_{d}/file_{f}.rs");
            }
        }
        assert_eq!(detect(&text), Some(Filter::Find));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.starts_with("75 paths in 25 directories"));
        assert!(out.contains("./src/dir_0/ (3): file_0.rs, file_1.rs, file_2.rs"));
        assert!(out.contains("… +5 more directories (15 paths)"), "{out}");
    }

    #[test]
    fn tree_hides_noise_directories_and_caps() {
        let mut text = String::from(".\n├── node_modules\n");
        for i in 0..50 {
            let _ = writeln!(text, "│   ├── package_{i}");
        }
        for i in 0..260 {
            let _ = writeln!(text, "├── file_{i}.rs");
        }
        text.push_str("\n12 directories, 300 files");
        assert_eq!(detect(&text), Some(Filter::Tree));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("├── node_modules (contents omitted)"));
        assert!(!out.contains("package_3"));
        assert!(out.contains("more entries"));
        assert!(out.contains("(50 entries inside omitted directories)"));
        assert!(out.ends_with("12 directories, 300 files"));
    }

    #[test]
    fn ls_summarises_files_and_extensions() {
        let mut text = String::from("total 128\n");
        text.push_str("drwxr-xr-x   5 dev  staff   160 Nov 10 12:00 src\n");
        text.push_str("drwxr-xr-x 900 dev  staff 28800 Nov 10 12:00 node_modules\n");
        for i in 0..12 {
            let _ = writeln!(
                text,
                "-rw-r--r--   1 dev  staff  1024 Nov 10 12:00 file_{i}.rs"
            );
        }
        text.push_str("-rw-r--r--   1 dev  staff  1024 Nov 10 12:00 README.md\n");
        assert_eq!(detect(&text), Some(Filter::Ls));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("dirs (1): src/"));
        assert!(out.contains("files (13): file_0.rs"));
        assert!(out.contains("by extension: .rs 12, .md 1"));
        assert!(out.contains("omitted: node_modules/"));
    }

    #[test]
    fn search_list_sorts_by_count() {
        let mut text = String::new();
        for i in 0..40 {
            let _ = writeln!(text, "src/some/long/directory/name/file_{i}.rs:{}", i % 7);
        }
        assert_eq!(detect(&text), Some(Filter::SearchList));
        let (out, _) = compress(&text).expect("compressed");
        assert!(
            out.lines().nth(1).is_some_and(|l| l.ends_with("(6)")),
            "{out}"
        );
        assert!(out.contains("… +10 more files"));
    }

    #[test]
    fn read_numbered_keeps_head_and_tail() {
        let text = (1..=400).fold(String::new(), |mut s, i| {
            let _ = writeln!(s, "{i:>6}\tline of source code {i}");
            s
        });
        assert_eq!(detect(&text), Some(Filter::ReadNumbered));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("   120\tline of source code 120"));
        assert!(!out.contains("source code 121\n"));
        assert!(out.contains("… 220 lines omitted …"));
        assert!(out.contains("   400\tline of source code 400"));
    }

    #[test]
    fn dedup_log_collapses_repeats() {
        let mut text = String::new();
        for i in 0..40 {
            let _ = writeln!(
                text,
                "2025-11-10T12:00:{:02}Z INFO polling upstream for changes, attempt {i}",
                i % 60
            );
        }
        text.push_str("2025-11-10T12:01:00Z ERROR upstream returned 503\n");
        assert_eq!(detect(&text), Some(Filter::DedupLog));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("(×40)"), "{out}");
        assert!(out.contains("ERROR upstream returned 503"));
    }

    #[test]
    fn smart_truncate_long_unique_output() {
        // Lines that differ in words, not just digits (digits normalise away).
        let word = |i: usize| -> String {
            let letters = b"abcdefghijklmnopqrstuvwxyz";
            format!(
                "{}{}{}",
                letters[i % 26] as char,
                letters[(i / 26) % 26] as char,
                letters[(i / 676) % 26] as char
            )
        };
        let text = (0..300).fold(String::new(), |mut s, i| {
            let _ = writeln!(s, "unique output line {}", word(i));
            s
        });
        assert_eq!(detect(&text), Some(Filter::SmartTruncate));
        let (out, _) = compress(&text).expect("compressed");
        assert!(out.contains("… 120 lines omitted …"));
        assert!(out.ends_with(&format!("unique output line {}", word(299))));
    }

    #[test]
    fn short_large_and_incompressible_results_are_skipped() {
        assert!(compress("short").is_none());
        let prose = "Plain prose without structure. ".repeat(30);
        assert!(compress(&prose).is_none(), "no detector matches");
        let huge = "a".repeat(MAX_BYTES + 1);
        assert!(compress(&huge).is_none());
    }
}
