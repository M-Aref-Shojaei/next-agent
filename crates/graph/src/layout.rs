// Ported from Logseq (AGPL-3.0, © Logseq contributors):
//   src/main/frontend/util/fs.cljs (tri-lb-file-name-sanity, ignored-path?)
//   deps/graph-parser/src/logseq/graph_parser/util.cljs (title-parsing)
//   deps/graph-parser/src/logseq/graph_parser/date_time_util.cljs
//   @ c67b8b5 (tag 0.10.9). Re-implemented in Rust.
//! Graph folder layout: `pages/`, `journals/`, `assets/`, `logseq/config.edn`,
//! triple-lowbar file names and journal date formats.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

use crate::parse::Document;

/// Logseq's config for graphs we create. Only the keys we rely on.
pub const DEFAULT_CONFIG_EDN: &str = r#";; Logseq graph config, created by Next Agent.
;; Format follows Logseq 0.10.9 logseq/config.edn; Logseq adds its other
;; defaults when it opens the graph.
{:meta/version 1
 :preferred-format :markdown
 :file/name-format :triple-lowbar
 :journal/page-title-format "MMM do, yyyy"
 :journal/file-name-format "yyyy_MM_dd"
 :hidden []
 :default-templates {:journals ""}}
"#;

pub const PAGES_DIR: &str = "pages";
pub const JOURNALS_DIR: &str = "journals";
pub const ASSETS_DIR: &str = "assets";
pub const CONFIG_PATH: &str = "logseq/config.edn";

// ----------------------------------------------------------------- dates --

/// A calendar date (proleptic Gregorian).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    #[must_use]
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        ((1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day))
            .then_some(Self { year, month, day })
    }

    /// From the leading `YYYY-MM-DD` of an ISO-8601 timestamp, as written
    /// (never shifted to another timezone).
    #[must_use]
    pub fn from_iso(iso: &str) -> Option<Self> {
        let head = iso.get(..10)?;
        let b = head.as_bytes();
        let ok = b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
        if !ok {
            return None;
        }
        Self::new(
            head[..4].parse().ok()?,
            head[5..7].parse().ok()?,
            head[8..10].parse().ok()?,
        )
    }

    /// `yyyyMMdd` as an integer (Logseq's `:block/journal-day`).
    #[must_use]
    pub fn as_int(self) -> i64 {
        i64::from(self.year) * 10_000 + i64::from(self.month) * 100 + i64::from(self.day)
    }

    #[must_use]
    pub fn from_int(n: i64) -> Option<Self> {
        Self::new(
            i32::try_from(n / 10_000).ok()?,
            u32::try_from(n / 100 % 100).ok()?,
            u32::try_from(n % 100).ok()?,
        )
    }

    /// 0 = Monday … 6 = Sunday.
    #[must_use]
    pub fn weekday(self) -> usize {
        // Sakamoto's method (0 = Sunday), shifted to Monday-first.
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let m = usize::try_from(self.month - 1).unwrap_or(0);
        let y = if self.month < 3 {
            self.year - 1
        } else {
            self.year
        };
        let day = i32::try_from(self.day).unwrap_or(1);
        let sunday_first = (y + y / 4 - y / 100 + y / 400 + T[m] + day).rem_euclid(7);
        usize::try_from((sunday_first + 6) % 7).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Year4,
    Year2,
    MonthName,
    MonthShort,
    Month2,
    Month1,
    Day2,
    Day1,
    DayOrdinal,
    WeekdayName,
    WeekdayShort,
    Literal(String),
}

fn tokenize(format: &str) -> Vec<Token> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut literal = String::new();
    let flush = |literal: &mut String, out: &mut Vec<Token>| {
        if !literal.is_empty() {
            out.push(Token::Literal(std::mem::take(literal)));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let run = chars[i..].iter().take_while(|x| **x == c).count();
        let token = match (c, run) {
            ('y', 4..) => Some(Token::Year4),
            ('y', _) => Some(Token::Year2),
            ('M', 4..) => Some(Token::MonthName),
            ('M', 3) => Some(Token::MonthShort),
            ('M', 2) => Some(Token::Month2),
            ('M', _) => Some(Token::Month1),
            ('d', 1) if chars.get(i + 1) == Some(&'o') => {
                flush(&mut literal, &mut out);
                out.push(Token::DayOrdinal);
                i += 2;
                continue;
            }
            ('d', 2..) => Some(Token::Day2),
            ('d', _) => Some(Token::Day1),
            ('E', 4..) => Some(Token::WeekdayName),
            ('E', _) => Some(Token::WeekdayShort),
            ('\'', _) => {
                // Quoted literal text.
                let end = chars[i + 1..].iter().position(|x| *x == '\'');
                let stop = end.map_or(chars.len(), |e| i + 1 + e);
                literal.extend(&chars[i + 1..stop]);
                i = stop + 1;
                continue;
            }
            _ => None,
        };
        if let Some(t) = token {
            flush(&mut literal, &mut out);
            out.push(t);
            i += run;
        } else {
            literal.push(c);
            i += 1;
        }
    }
    flush(&mut literal, &mut out);
    out
}

fn ordinal(day: u32) -> &'static str {
    match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

/// Format a date with a Logseq (Joda/cljs-time style) pattern such as
/// `yyyy_MM_dd` or `MMM do, yyyy`.
#[must_use]
pub fn format_date(format: &str, date: Date) -> String {
    let month = MONTHS[usize::try_from(date.month - 1).unwrap_or(0)];
    let weekday = WEEKDAYS[date.weekday()];
    tokenize(format)
        .into_iter()
        .map(|t| match t {
            Token::Year4 => format!("{:04}", date.year),
            Token::Year2 => format!("{:02}", date.year.rem_euclid(100)),
            Token::MonthName => month.to_string(),
            Token::MonthShort => month[..3].to_string(),
            Token::Month2 => format!("{:02}", date.month),
            Token::Month1 => date.month.to_string(),
            Token::Day2 => format!("{:02}", date.day),
            Token::Day1 => date.day.to_string(),
            Token::DayOrdinal => format!("{}{}", date.day, ordinal(date.day)),
            Token::WeekdayName => weekday.to_string(),
            Token::WeekdayShort => weekday[..3].to_string(),
            Token::Literal(s) => s,
        })
        .collect()
}

fn take_digits(s: &str, min: usize, max: usize) -> Option<(u32, &str)> {
    let n = s.bytes().take(max).take_while(u8::is_ascii_digit).count();
    (n >= min).then(|| Some((s[..n].parse().ok()?, &s[n..])))?
}

fn take_name<'a>(s: &'a str, names: &[&str], short: bool) -> Option<(u32, &'a str)> {
    for (i, name) in names.iter().enumerate() {
        let candidate = if short { &name[..3] } else { name };
        if s.len() >= candidate.len()
            && s.is_char_boundary(candidate.len())
            && s[..candidate.len()].eq_ignore_ascii_case(candidate)
        {
            return Some((u32::try_from(i).ok()? + 1, &s[candidate.len()..]));
        }
    }
    None
}

/// Parse text written with [`format_date`]'s patterns.
#[must_use]
pub fn parse_date(format: &str, text: &str) -> Option<Date> {
    let (mut year, mut month, mut day) = (None, None, None);
    let mut rest = text;
    for token in tokenize(format) {
        rest = match token {
            Token::Year4 => {
                let (v, r) = take_digits(rest, 4, 4)?;
                year = Some(i32::try_from(v).ok()?);
                r
            }
            Token::Year2 => {
                let (v, r) = take_digits(rest, 2, 2)?;
                year = Some(2000 + i32::try_from(v).ok()?);
                r
            }
            Token::MonthName | Token::MonthShort => {
                let (v, r) = take_name(rest, &MONTHS, token == Token::MonthShort)?;
                month = Some(v);
                r
            }
            Token::Month2 => {
                let (v, r) = take_digits(rest, 2, 2)?;
                month = Some(v);
                r
            }
            Token::Month1 => {
                let (v, r) = take_digits(rest, 1, 2)?;
                month = Some(v);
                r
            }
            Token::Day2 => {
                let (v, r) = take_digits(rest, 2, 2)?;
                day = Some(v);
                r
            }
            Token::Day1 => {
                let (v, r) = take_digits(rest, 1, 2)?;
                day = Some(v);
                r
            }
            Token::DayOrdinal => {
                let (v, r) = take_digits(rest, 1, 2)?;
                day = Some(v);
                let suffix = r.get(..2)?;
                if !["st", "nd", "rd", "th"].contains(&suffix.to_ascii_lowercase().as_str()) {
                    return None;
                }
                &r[2..]
            }
            Token::WeekdayName | Token::WeekdayShort => {
                take_name(rest, &WEEKDAYS, token == Token::WeekdayShort)?.1
            }
            Token::Literal(lit) => {
                if rest.len() < lit.len()
                    || !rest.is_char_boundary(lit.len())
                    || !rest[..lit.len()].eq_ignore_ascii_case(&lit)
                {
                    return None;
                }
                &rest[lit.len()..]
            }
        };
    }
    if !rest.is_empty() {
        return None;
    }
    Date::new(year?, month?, day?)
}

// ------------------------------------------------------------ file names --

const RESERVED: &[char] = &[':', '*', '?', '"', '<', '>', '|', '#', '\\'];
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_hex_escape(b: &[u8]) -> bool {
    b.len() >= 3 && b[0] == b'%' && b[1].is_ascii_hexdigit() && b[2].is_ascii_hexdigit()
}

/// File body (no extension) for a page title, `:file/name-format
/// :triple-lowbar` (`tri-lb-file-name-sanity`). `/` becomes `___`.
#[must_use]
pub fn page_file_body(title: &str) -> String {
    // page-name-sanity: drop one boundary slash on each side.
    let mut t = title;
    t = t.strip_prefix('/').unwrap_or(t);
    t = t.strip_suffix('/').unwrap_or(t);

    // Pre-encode `%` that already looks like an escape, then reserved chars.
    let bytes = t.as_bytes();
    let mut s = String::with_capacity(t.len());
    for (i, ch) in t.char_indices() {
        if ch == '%' && is_hex_escape(&bytes[i..]) {
            s.push_str("%25");
        } else if RESERVED.contains(&ch) {
            let _ = write!(s, "%{:02X}", u32::from(ch));
        } else {
            s.push(ch);
        }
    }
    if let Some(rest) = s.strip_prefix('.') {
        s = format!("%2E{rest}");
    }
    if WINDOWS_RESERVED.contains(&s.as_str()) || s.ends_with('.') {
        s.push('/');
    }
    s.replace("___", "%5F%5F%5F")
        .replace("_/", "%5F/")
        .replace("/_", "/%5F")
        .replace('/', "___")
}

/// `pages/<body>.md` relative path for a page title.
#[must_use]
pub fn page_file_name(title: &str) -> String {
    format!("{}.md", page_file_body(title))
}

/// Page title for a file body under `:triple-lowbar` (`tri-lb-title-parsing`).
#[must_use]
pub fn title_from_file_body(body: &str) -> String {
    let slashed = body.replace("___", "/");
    let b = slashed.as_bytes();
    let mut out = String::with_capacity(slashed.len());
    let mut i = 0;
    while i < b.len() {
        if is_hex_escape(&b[i..]) {
            let v = u8::from_str_radix(&slashed[i + 1..i + 3], 16).unwrap_or(0);
            if v.is_ascii() {
                out.push(char::from(v));
                i += 3;
                continue;
            }
        }
        let ch = slashed[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    // make-valid-namespaces: drop empty namespace parts.
    out.split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Page title for a legacy (pre-`:triple-lowbar`) file body: `.` means `/`
/// and the name is URL-decoded (`legacy-title-parsing`).
#[must_use]
pub fn legacy_title_from_file_body(body: &str) -> String {
    let dotted = body.replace('.', "/");
    percent_decode(&dotted).unwrap_or(dotted)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if !is_hex_escape(&b[i..]) {
                return None;
            }
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// True when Logseq would add a `title::` property for this page because
/// the file name cannot carry the title exactly (`create-title-property?`).
#[must_use]
pub fn needs_title_property(title: &str) -> bool {
    let body = page_file_body(title);
    title_from_file_body(&body) != title || title.contains(RESERVED)
}

// ---------------------------------------------------------------- config --

/// Which file name rules the graph uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileNameFormat {
    TripleLowbar,
    Legacy,
}

/// The parts of `logseq/config.edn` the engine honours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphConfig {
    pub journal_file_format: String,
    pub journal_title_format: String,
    pub file_name_format: FileNameFormat,
    /// `:hidden` paths (relative to the graph root) to skip.
    pub hidden: Vec<String>,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            journal_file_format: "yyyy_MM_dd".into(),
            journal_title_format: "MMM do, yyyy".into(),
            file_name_format: FileNameFormat::TripleLowbar,
            hidden: Vec::new(),
        }
    }
}

fn strip_edn_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    for ch in text.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                out.push(ch);
            }
            continue;
        }
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            ';' => in_comment = true,
            '"' => {
                in_string = true;
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Raw EDN token following `key` (a string with quotes, a vector, or a
/// keyword/symbol).
fn edn_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(at) = text[from..].find(key) {
        let start = from + at + key.len();
        from = start;
        // Must be a whole keyword.
        if text[start..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != ',')
        {
            continue;
        }
        let rest = text[start..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        let offset = text.len() - rest.len();
        return match rest.chars().next()? {
            '"' => {
                let mut escaped = false;
                for (i, ch) in rest.char_indices().skip(1) {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        return Some(&text[offset..=offset + i]);
                    }
                }
                None
            }
            '[' => rest.find(']').map(|e| &text[offset..=offset + e]),
            _ => {
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '}' || c == ']' || c == ',')
                    .unwrap_or(rest.len());
                Some(&text[offset..offset + end])
            }
        };
    }
    None
}

fn edn_string(token: &str) -> Option<String> {
    let inner = token.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

impl GraphConfig {
    /// Read the keys we use from `config.edn` text. Missing keys get
    /// Logseq's defaults; a missing `:file/name-format` means the legacy
    /// format, as in Logseq.
    #[must_use]
    pub fn from_edn(text: &str) -> Self {
        let text = strip_edn_comments(text);
        let mut config = Self {
            file_name_format: FileNameFormat::Legacy,
            ..Self::default()
        };
        if let Some(v) = edn_value(&text, ":journal/file-name-format").and_then(edn_string) {
            config.journal_file_format = v;
        }
        if let Some(v) = edn_value(&text, ":journal/page-title-format").and_then(edn_string) {
            config.journal_title_format = v;
        }
        if edn_value(&text, ":file/name-format") == Some(":triple-lowbar") {
            config.file_name_format = FileNameFormat::TripleLowbar;
        }
        if let Some(v) = edn_value(&text, ":hidden") {
            let mut items = Vec::new();
            let mut rest = v;
            while let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                let Some(end) = after.find('"') else { break };
                let item = after[..end].trim_matches('/');
                if !item.is_empty() {
                    items.push(item.to_string());
                }
                rest = &after[end + 1..];
            }
            config.hidden = items;
        }
        config
    }
}

// ----------------------------------------------------------------- graph --

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph folder not found: {0}")]
    NotFound(PathBuf),
    #[error("path escapes the graph: {0}")]
    BadPath(String),
    #[error("graph I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// What a graph file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Page,
    Journal(Date),
}

/// A Logseq Markdown graph on disk.
#[derive(Debug, Clone)]
pub struct Graph {
    root: PathBuf,
    config: GraphConfig,
}

/// Write via a temp file in the same folder + rename, so readers see the
/// old file or the new one, never a partial write.
///
/// # Errors
/// Any I/O failure; the temp file is removed on failure.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

impl Graph {
    /// Open an existing folder as a graph. A missing `logseq/config.edn` is
    /// fine: defaults are used (and [`Graph::init`] can create one).
    ///
    /// # Errors
    /// [`GraphError::NotFound`] when `root` is not a folder.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GraphError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(GraphError::NotFound(root));
        }
        let config = match fs::read_to_string(root.join(CONFIG_PATH)) {
            Ok(text) => GraphConfig::from_edn(&text),
            Err(_) => GraphConfig::default(),
        };
        Ok(Self { root, config })
    }

    /// Create the graph folders and a minimal `logseq/config.edn` when
    /// missing. Existing files are never touched. Returns the graph and
    /// whether the config file was created.
    ///
    /// # Errors
    /// Any I/O failure.
    pub fn init(root: impl Into<PathBuf>) -> Result<(Self, bool), GraphError> {
        let root = root.into();
        for dir in [PAGES_DIR, JOURNALS_DIR, ASSETS_DIR, "logseq"] {
            fs::create_dir_all(root.join(dir))?;
        }
        let config_path = root.join(CONFIG_PATH);
        let created = if config_path.exists() {
            false
        } else {
            write_atomic(&config_path, DEFAULT_CONFIG_EDN)?;
            true
        };
        Ok((Self::open(root)?, created))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn config(&self) -> &GraphConfig {
        &self.config
    }

    /// Re-read `logseq/config.edn`.
    pub fn reload_config(&mut self) {
        self.config = match fs::read_to_string(self.root.join(CONFIG_PATH)) {
            Ok(text) => GraphConfig::from_edn(&text),
            Err(_) => GraphConfig::default(),
        };
    }

    /// Journal page title, e.g. `Nov 14th, 2026`.
    #[must_use]
    pub fn journal_title(&self, date: Date) -> String {
        format_date(&self.config.journal_title_format, date)
    }

    /// `journals/2026_11_14.md`.
    #[must_use]
    pub fn journal_rel_path(&self, date: Date) -> String {
        format!(
            "{JOURNALS_DIR}/{}.md",
            format_date(&self.config.journal_file_format, date)
        )
    }

    /// `pages/<escaped title>.md`.
    #[must_use]
    pub fn page_rel_path(&self, title: &str) -> String {
        format!("{PAGES_DIR}/{}", page_file_name(title))
    }

    /// Absolute path for a graph-relative path, refusing anything that
    /// would leave the graph folder.
    ///
    /// # Errors
    /// [`GraphError::BadPath`] for absolute paths or `..`.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, GraphError> {
        let path = Path::new(rel);
        if rel.is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(GraphError::BadPath(rel.to_string()));
        }
        Ok(self.root.join(path))
    }

    /// True for paths Logseq skips (`ignored-path?` and `:hidden`).
    #[must_use]
    pub fn is_ignored(&self, rel: &str) -> bool {
        let rel = rel.trim_start_matches('/');
        rel.split('/')
            .any(|part| part.starts_with('.') || part == "node_modules")
            || ["logseq/bak", "logseq/version-files"]
                .iter()
                .any(|p| rel.starts_with(p))
            || self
                .config
                .hidden
                .iter()
                .any(|h| rel == h || rel.starts_with(&format!("{h}/")))
    }

    /// Classify a graph-relative path (`/` separators).
    #[must_use]
    pub fn classify(&self, rel: &str) -> Option<FileKind> {
        if self.is_ignored(rel) {
            return None;
        }
        let ext = Path::new(rel)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        let stem = match ext.as_deref() {
            Some("md") => &rel[..rel.len() - 3],
            Some("markdown") => &rel[..rel.len() - 9],
            _ => return None,
        };
        if let Some(name) = stem.strip_prefix("journals/") {
            if name.contains('/') {
                return None;
            }
            return parse_date(&self.config.journal_file_format, name)
                .or_else(|| parse_date("yyyy_MM_dd", name))
                .map(FileKind::Journal);
        }
        stem.strip_prefix("pages/").map(|_| FileKind::Page)
    }

    /// The page name for a file: `title::` first, then the journal date,
    /// then the file name.
    #[must_use]
    pub fn page_name(&self, rel: &str, doc: &Document) -> String {
        if let Some(title) = doc
            .page_properties()
            .into_iter()
            .find(|p| p.key == "title" && !p.value.trim().is_empty())
        {
            return title.value.trim().to_string();
        }
        if let Some(FileKind::Journal(date)) = self.classify(rel) {
            return self.journal_title(date);
        }
        let file = rel.rsplit('/').next().unwrap_or(rel);
        let body = file
            .strip_suffix(".md")
            .or_else(|| file.strip_suffix(".markdown"))
            .unwrap_or(file);
        match self.config.file_name_format {
            FileNameFormat::TripleLowbar => title_from_file_body(body),
            FileNameFormat::Legacy => legacy_title_from_file_body(body),
        }
    }

    /// Every page and journal file, graph-relative, sorted.
    ///
    /// # Errors
    /// I/O failures while listing.
    pub fn files(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        for dir in [PAGES_DIR, JOURNALS_DIR] {
            self.walk(&self.root.join(dir), &mut out)?;
        }
        out.sort();
        Ok(out)
    }

    fn walk(&self, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let Some(rel) = self.relative(&path) else {
                continue;
            };
            if self.is_ignored(&rel) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                self.walk(&path, out)?;
            } else if self.classify(&rel).is_some() {
                out.push(rel);
            }
        }
        Ok(())
    }

    /// Graph-relative `/`-separated path for an absolute path inside it.
    #[must_use]
    pub fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    /// Read and parse a file.
    ///
    /// # Errors
    /// Bad path or I/O failure.
    pub fn read(&self, rel: &str) -> Result<Document, GraphError> {
        let text = fs::read_to_string(self.resolve(rel)?)?;
        Ok(Document::parse(&text))
    }

    /// Write a document atomically. Returns `false` (and writes nothing)
    /// when the file already has exactly this content.
    ///
    /// # Errors
    /// Bad path or I/O failure.
    pub fn write(&self, rel: &str, doc: &Document) -> Result<bool, GraphError> {
        self.write_text(rel, &doc.render())
    }

    /// Write text atomically unless the file already holds it.
    ///
    /// # Errors
    /// Bad path or I/O failure.
    pub fn write_text(&self, rel: &str, text: &str) -> Result<bool, GraphError> {
        let path = self.resolve(rel)?;
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            return Ok(false);
        }
        write_atomic(&path, text)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_lowbar_matches_logseq_rules() {
        let cases = [
            (
                "Next Agent/my-app/Tasks/x",
                "Next Agent___my-app___Tasks___x",
            ),
            ("a:b", "a%3Ab"),
            ("what?", "what%3F"),
            ("100%", "100%"),
            ("50%25 off", "50%2525 off"),
            (".hidden", "%2Ehidden"),
            ("CON", "CON___"),
            ("end.", "end.___"),
            ("a___b", "a%5F%5F%5Fb"),
            ("a_/b", "a%5F___b"),
            ("/lead/", "lead"),
            ("c#", "c%23"),
            ("日本語/ページ", "日本語___ページ"),
        ];
        for (title, body) in cases {
            assert_eq!(page_file_body(title), body, "{title}");
        }
        for (title, body) in cases {
            let back = title_from_file_body(body);
            if !matches!(title, "/lead/" | "CON" | "end.") {
                assert_eq!(back, title, "{body}");
            }
        }
        assert_eq!(title_from_file_body("CON___"), "CON");
        assert!(needs_title_property("a:b"));
        assert!(!needs_title_property("Plain/Name"));
    }

    #[test]
    fn legacy_names() {
        assert_eq!(legacy_title_from_file_body("a.b%3Ac"), "a/b:c");
    }

    #[test]
    fn dates_format_and_parse() {
        let d = Date::new(2026, 11, 14).expect("date");
        assert_eq!(d.weekday(), 5); // Saturday
        assert_eq!(format_date("yyyy_MM_dd", d), "2026_11_14");
        assert_eq!(format_date("MMM do, yyyy", d), "Nov 14th, 2026");
        assert_eq!(format_date("EEE, dd.MM.yyyy", d), "Sat, 14.11.2026");
        assert_eq!(
            format_date("EEEE, MMMM d'st' yy", d),
            "Saturday, November 14st 26"
        );
        assert_eq!(parse_date("yyyy_MM_dd", "2026_11_14"), Some(d));
        assert_eq!(parse_date("MMM do, yyyy", "Nov 14th, 2026"), Some(d));
        assert_eq!(parse_date("yyyy-MM-dd", "2026-02-30"), None);
        assert_eq!(parse_date("yyyy_MM_dd", "2026_11_14x"), None);
        for (day, s) in [
            (1, "1st"),
            (2, "2nd"),
            (3, "3rd"),
            (11, "11th"),
            (22, "22nd"),
            (13, "13th"),
        ] {
            let d = Date::new(2026, 1, day).expect("d");
            assert_eq!(format_date("do", d), s);
        }
        assert_eq!(
            Date::from_iso("2026-12-31T23:59:59-08:00"),
            Date::new(2026, 12, 31)
        );
        assert_eq!(Date::from_int(20_261_114), Some(d));
        assert_eq!(d.as_int(), 20_261_114);
    }

    #[test]
    fn config_edn_keys_and_comments() {
        let edn = r#"{:meta/version 1
 ;; :journal/file-name-format "nope"
 :journal/page-title-format "yyyy-MM-dd" ; trailing ; comment
 :journal/file-name-format "yyyy-MM-dd"
 :file/name-format :triple-lowbar
 :hidden ["/archive" "drafts/"]
 :x "a;b"}"#;
        let c = GraphConfig::from_edn(edn);
        assert_eq!(c.journal_file_format, "yyyy-MM-dd");
        assert_eq!(c.journal_title_format, "yyyy-MM-dd");
        assert_eq!(c.file_name_format, FileNameFormat::TripleLowbar);
        assert_eq!(c.hidden, ["archive", "drafts"]);
        assert_eq!(
            GraphConfig::from_edn("{}").file_name_format,
            FileNameFormat::Legacy
        );
        assert_eq!(
            GraphConfig::from_edn(DEFAULT_CONFIG_EDN),
            GraphConfig::default()
        );
    }

    #[test]
    fn classify_and_resolve() {
        let dir = tempfile::tempdir().expect("tmp");
        let (g, created) = Graph::init(dir.path()).expect("init");
        assert!(created);
        assert!(!Graph::init(dir.path()).expect("again").1);
        assert_eq!(g.classify("pages/a.md"), Some(FileKind::Page));
        assert_eq!(
            g.classify("journals/2026_11_14.md"),
            Some(FileKind::Journal(Date::new(2026, 11, 14).expect("d")))
        );
        assert_eq!(g.classify("journals/notes.md"), None);
        assert_eq!(g.classify("pages/.hidden.md"), None);
        assert_eq!(g.classify("logseq/bak/pages/a.md"), None);
        assert_eq!(g.classify("pages/a.png"), None);
        assert!(g.resolve("../x").is_err());
        assert!(g.resolve("/etc/passwd").is_err());
        assert!(g.resolve("pages/ok.md").is_ok());
    }
}
