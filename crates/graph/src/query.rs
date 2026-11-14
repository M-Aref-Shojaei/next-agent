// Behaviour follows Logseq (AGPL-3.0, © Logseq contributors): simple queries,
// frontend/db/query_dsl.cljs @ c67b8b5 (tag 0.10.9). We compile the
// `{{query ...}}` s-expression to SQL over our index instead of Datalog.
//! Simple queries, the all-pages list and the graph-view data.
//!
//! Query grammar (an `and` is implied at the top level):
//!
//! ```text
//! [[page]] | "text"          blocks that reference the page / contain the text
//! (and q…) (or q…) (not q)
//! (task TODO DOING …)        task marker
//! (priority A B …)           priority [#A]
//! (page "name")              blocks on that page
//! (property key [value])     block property, optionally with a value
//! (between A B)              blocks on journal days A..=B; A and B are
//!                            [[journal pages]], `today`, `yesterday`,
//!                            `tomorrow`, or `-7d` / `+2w` / `-1m` / `-1y`
//! ```
//!
//! Every value reaches SQLite as a bound parameter. This module only builds the SQL
//! condition; `index.rs` is the one file that talks to SQLite and runs it.

use serde::Serialize;

use crate::index::IndexError;
use crate::layout::Date;

type Result<T> = std::result::Result<T, IndexError>;

const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 200;

/// What a query needs from the outside: today's date and the graph's
/// journal title format.
pub struct QueryEnv<'a> {
    pub today: Date,
    pub parse_journal: &'a dyn Fn(&str) -> Option<Date>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Open,
    Close,
    Link(String),
    Str(String),
    Word(String),
}

fn bad(msg: impl Into<String>) -> IndexError {
    IndexError::Query(msg.into())
}

fn lex(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            out.push(Tok::Open);
            i += 1;
        } else if c == ')' {
            out.push(Tok::Close);
            i += 1;
        } else if c == '[' && chars.get(i + 1) == Some(&'[') {
            let start = i + 2;
            let mut j = start;
            while j + 1 < chars.len() && !(chars[j] == ']' && chars[j + 1] == ']') {
                j += 1;
            }
            if j + 1 >= chars.len() {
                return Err(bad("unclosed [["));
            }
            out.push(Tok::Link(chars[start..j].iter().collect()));
            i = j + 2;
        } else if c == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            while j < chars.len() && chars[j] != '"' {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    j += 1;
                }
                s.push(chars[j]);
                j += 1;
            }
            if j >= chars.len() {
                return Err(bad("unclosed string"));
            }
            out.push(Tok::Str(s));
            i = j + 1;
        } else {
            let mut j = i;
            while j < chars.len() && !chars[j].is_whitespace() && !"()\"".contains(chars[j]) {
                j += 1;
            }
            out.push(Tok::Word(chars[i..j].iter().collect()));
            i = j;
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Ref(String),
    Text(String),
    Task(Vec<String>),
    Priority(Vec<String>),
    Page(String),
    Property(String, Option<String>),
    Between(String, String),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    nodes: usize,
}

impl Parser {
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// Operands up to the closing paren (consumed).
    fn args(&mut self, depth: usize) -> Result<Vec<Node>> {
        let mut out = Vec::new();
        loop {
            if self.peek() == Some(&Tok::Close) {
                self.pos += 1;
                return Ok(out);
            }
            out.push(self.node(depth)?);
        }
    }

    /// Plain values (words, strings, links) up to the closing paren.
    fn values(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            match self.next() {
                Some(Tok::Close) => return Ok(out),
                Some(Tok::Word(s) | Tok::Str(s) | Tok::Link(s)) => out.push(s),
                Some(Tok::Open) => return Err(bad("unexpected ( in a value list")),
                None => return Err(bad("missing )")),
            }
        }
    }

    fn node(&mut self, depth: usize) -> Result<Node> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(bad("query is too large"));
        }
        match self.next() {
            Some(Tok::Link(s)) => Ok(Node::Ref(s)),
            Some(Tok::Str(s)) => Ok(Node::Text(s)),
            Some(Tok::Word(w)) => Ok(Node::Text(w)),
            Some(Tok::Open) => {
                let Some(Tok::Word(op)) = self.next() else {
                    return Err(bad("expected an operator after ("));
                };
                match op.to_lowercase().as_str() {
                    "and" => Ok(Node::And(self.args(depth + 1)?)),
                    "or" => Ok(Node::Or(self.args(depth + 1)?)),
                    "not" => {
                        let mut a = self.args(depth + 1)?;
                        if a.len() == 1 {
                            Ok(Node::Not(Box::new(a.remove(0))))
                        } else {
                            Ok(Node::Not(Box::new(Node::Or(a))))
                        }
                    }
                    "task" => Ok(Node::Task(
                        self.values()?.iter().map(|s| s.to_uppercase()).collect(),
                    )),
                    "priority" => Ok(Node::Priority(
                        self.values()?.iter().map(|s| s.to_uppercase()).collect(),
                    )),
                    "page" => match self.values()?.as_slice() {
                        [name] => Ok(Node::Page(name.clone())),
                        _ => Err(bad("(page \"name\") takes one name")),
                    },
                    "property" => match self.values()?.as_slice() {
                        [key] => Ok(Node::Property(key.clone(), None)),
                        [key, value] => Ok(Node::Property(key.clone(), Some(value.clone()))),
                        _ => Err(bad("(property key [value])")),
                    },
                    "between" => match self.values()?.as_slice() {
                        [a, b] => Ok(Node::Between(a.clone(), b.clone())),
                        _ => Err(bad("(between start end)")),
                    },
                    other => Err(bad(format!("unknown operator: {other}"))),
                }
            }
            Some(Tok::Close) => Err(bad("unexpected )")),
            None => Err(bad("query ended early")),
        }
    }
}

fn parse(src: &str) -> Result<Node> {
    // Accept the whole `{{query …}}` macro as written in a block.
    let src = src.trim();
    let src = src
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .map_or(src, str::trim);
    let src = src
        .strip_prefix("query")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .unwrap_or(src);
    let mut p = Parser {
        toks: lex(src)?,
        pos: 0,
        nodes: 0,
    };
    let mut parts = Vec::new();
    while p.peek().is_some() {
        parts.push(p.node(0)?);
    }
    match parts.len() {
        0 => Err(bad("empty query")),
        1 => Ok(parts.remove(0)),
        _ => Ok(Node::And(parts)),
    }
}

// ------------------------------------------------------------------ dates --

fn days_from_civil(d: Date) -> i64 {
    let y = i64::from(d.year) - i64::from(d.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(d.month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> Option<Date> {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Date::new(
        i32::try_from(y).ok()?,
        u32::try_from(m).ok()?,
        u32::try_from(d).ok()?,
    )
}

fn add_months(d: Date, months: i64) -> Option<Date> {
    let total = i64::from(d.year) * 12 + i64::from(d.month) - 1 + months;
    let (year, month) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let month = u32::try_from(month).ok()?;
    let year = i32::try_from(year).ok()?;
    // Clamp the day (Jan 31 + 1 month → Feb 28/29).
    (1..=d.day)
        .rev()
        .find_map(|day| Date::new(year, month, day))
}

fn resolve_day(arg: &str, env: &QueryEnv) -> Result<i64> {
    let lower = arg.trim().to_lowercase();
    let shift = |days: i64| {
        civil_from_days(days_from_civil(env.today) + days)
            .map(Date::as_int)
            .ok_or_else(|| bad(format!("date out of range: {arg}")))
    };
    match lower.as_str() {
        "today" => return Ok(env.today.as_int()),
        "yesterday" => return shift(-1),
        "tomorrow" => return shift(1),
        _ => {}
    }
    let relative = lower
        .strip_prefix(['-', '+'])
        .and_then(|rest| {
            let unit = rest.chars().last()?;
            let n: i64 = rest[..rest.len() - unit.len_utf8()].parse().ok()?;
            Some((n, unit))
        })
        .map(|(n, unit)| (if lower.starts_with('-') { -n } else { n }, unit));
    if let Some((n, unit)) = relative {
        let months = |m: i64| {
            add_months(env.today, m)
                .map(Date::as_int)
                .ok_or_else(|| bad(format!("date out of range: {arg}")))
        };
        return match unit {
            'd' => shift(n),
            'w' => shift(n * 7),
            'm' => months(n),
            'y' => months(n * 12),
            _ => Err(bad(format!("bad relative date: {arg}"))),
        };
    }
    (env.parse_journal)(arg)
        .map(Date::as_int)
        .ok_or_else(|| bad(format!("not a journal day: {arg}")))
}

// -------------------------------------------------------------------- sql --

/// A bound SQL parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Text(String),
    Int(i64),
}

impl From<String> for Arg {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
impl From<i64> for Arg {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

/// Lowercased names that mean a page (itself and its aliases).
pub type ResolveNames<'a> = &'a dyn Fn(&str) -> Result<Vec<String>>;

/// A `WHERE` condition over `blocks b JOIN pages p`, with its parameters (`?1`, `?2`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub condition: String,
    pub args: Vec<Arg>,
}

/// Parse `text` and compile it to SQL.
///
/// # Errors
/// [`IndexError::Query`] when the query does not parse.
pub fn compile(text: &str, env: &QueryEnv, resolve: ResolveNames) -> Result<Compiled> {
    let tree = parse(text)?;
    let mut sql = Sql {
        resolve,
        env,
        params: Vec::new(),
    };
    let condition = sql.compile(&tree)?;
    Ok(Compiled {
        condition,
        args: sql.params,
    })
}

struct Sql<'a> {
    resolve: ResolveNames<'a>,
    env: &'a QueryEnv<'a>,
    params: Vec<Arg>,
}

impl Sql<'_> {
    fn bind(&mut self, v: impl Into<Arg>) -> String {
        self.params.push(v.into());
        format!("?{}", self.params.len())
    }

    fn list(&mut self, values: &[String]) -> String {
        let marks: Vec<String> = values.iter().map(|v| self.bind(v.clone())).collect();
        marks.join(", ")
    }

    fn group(&mut self, nodes: &[Node], glue: &str, empty: &str) -> Result<String> {
        if nodes.is_empty() {
            return Ok(empty.to_string());
        }
        let parts = nodes
            .iter()
            .map(|n| self.compile(n))
            .collect::<Result<Vec<_>>>()?;
        Ok(format!("({})", parts.join(glue)))
    }

    fn compile(&mut self, node: &Node) -> Result<String> {
        Ok(match node {
            Node::And(v) => self.group(v, " AND ", "1")?,
            Node::Or(v) => self.group(v, " OR ", "0")?,
            Node::Not(n) => format!("NOT {}", self.compile(n)?),
            Node::Ref(name) => {
                let names = (self.resolve)(name)?;
                let marks = self.list(&names);
                format!(
                    "EXISTS (SELECT 1 FROM refs r WHERE r.src_block = b.id
                             AND r.dst_page_lc IN ({marks}))"
                )
            }
            Node::Text(text) => {
                let escaped = text
                    .to_lowercase()
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let m = self.bind(format!("%{escaped}%"));
                format!("lower(b.content) LIKE {m} ESCAPE '\\'")
            }
            Node::Task(markers) => format!("b.marker IN ({})", self.list(markers)),
            Node::Priority(levels) => format!("b.priority IN ({})", self.list(levels)),
            Node::Page(name) => {
                let m = self.bind(name.trim().to_lowercase());
                format!("p.name_lc = {m}")
            }
            Node::Property(key, value) => {
                let key = key
                    .trim_start_matches(':')
                    .to_lowercase()
                    .replace(['_', ' '], "-");
                let k = self.bind(key);
                let mut sql = format!(
                    "EXISTS (SELECT 1 FROM json_each(b.properties) j
                             WHERE json_extract(j.value, '$.key') = {k}"
                );
                if let Some(v) = value {
                    let plain = self.bind(v.to_lowercase());
                    let link = self.bind(format!("[[{}]]", v.to_lowercase()));
                    sql = format!(
                        "{sql} AND (lower(json_extract(j.value, '$.value')) = {plain}
                              OR instr(lower(json_extract(j.value, '$.value')), {link}) > 0)"
                    );
                }
                sql.push(')');
                sql
            }
            Node::Between(a, b) => {
                let (lo, hi) = (resolve_day(a, self.env)?, resolve_day(b, self.env)?);
                let (lo, hi) = (lo.min(hi), lo.max(hi));
                let (lo, hi) = (self.bind(lo), self.bind(hi));
                format!("p.journal_day BETWEEN {lo} AND {hi}")
            }
        })
    }
}

/// One row of the all-pages list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageListItem {
    pub name: String,
    pub journal_day: Option<i64>,
    pub blocks: i64,
    pub backlinks: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Lowercased page name.
    pub id: String,
    pub name: String,
    pub journal: bool,
    pub missing: bool,
    pub degree: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::parse_date;

    fn env() -> QueryEnv<'static> {
        QueryEnv {
            today: Date::new(2026, 11, 14).expect("date"),
            parse_journal: &|s| parse_date("MMM do, yyyy", s),
        }
    }

    #[test]
    fn parses_the_macro_and_operators() {
        let n = parse("{{query (and [[x]] (task TODO doing))}}").expect("parse");
        assert_eq!(
            n,
            Node::And(vec![
                Node::Ref("x".into()),
                Node::Task(vec!["TODO".into(), "DOING".into()])
            ])
        );
        assert_eq!(
            parse("[[a]] \"b c\"").expect("implicit and"),
            Node::And(vec![Node::Ref("a".into()), Node::Text("b c".into())])
        );
        for bad in [
            "",
            "(",
            ")",
            "(nope x)",
            "[[open",
            "(page)",
            "(and (not",
            "(task (x))",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = format!("{}x{}", "(not ".repeat(40), ")".repeat(40));
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn relative_dates() {
        let e = env();
        assert_eq!(resolve_day("today", &e).expect("d"), 20_261_114);
        assert_eq!(resolve_day("yesterday", &e).expect("d"), 20_261_113);
        assert_eq!(resolve_day("-14d", &e).expect("d"), 20_261_031);
        assert_eq!(resolve_day("+2w", &e).expect("d"), 20_261_128);
        assert_eq!(resolve_day("-1m", &e).expect("d"), 20_261_014);
        assert_eq!(resolve_day("+1y", &e).expect("d"), 20_271_114);
        assert_eq!(resolve_day("Nov 3rd, 2026", &e).expect("d"), 20_261_103);
        assert!(resolve_day("whenever", &e).is_err());
        // Month-end clamp.
        let jan31 = QueryEnv {
            today: Date::new(2026, 1, 31).expect("date"),
            ..env()
        };
        assert_eq!(resolve_day("+1m", &jan31).expect("d"), 20_260_228);
    }
}
