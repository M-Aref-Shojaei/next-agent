//! Shell command classification.
//!
//! M1 does not run commands itself, it decides whether a command *may* run.
//! So this module errs toward the more restrictive class:
//!
//! - a POSIX-ish parser splits the line on `|`, `||`, `&&`, `&`, `;`,
//!   newlines, subshells `( )`, command substitution `$( )` / backticks
//!   (including inside double quotes and `${…}`), process substitution
//!   `<( )`/`>( )`, and records redirections and here-documents;
//! - every simple command is classified and **the most restrictive wins**;
//! - wrappers (`sudo`, `env`, `nohup`, `timeout`, `xargs`, `uv run`, …) are
//!   looked through, and inline scripts (`sh -c`, `python -c`, `node -e`,
//!   `eval`) are classified recursively with a floor of `unknown`;
//! - anything the parser cannot follow (unbalanced quotes, dynamic program
//!   names, parameter expansion) is at least `unknown`, never `safe_dev`.
//!
//! The classifier is a policy aid, not a sandbox. A permitted program can still
//! run arbitrary code (risk R1/R2 in `Risks and Open Questions`).

/// Risk class of a command. Ordered from least to most restrictive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandClass {
    /// Ordinary development work: builds, tests, reads, `git status`.
    SafeDev,
    /// Not recognised as safe; may still be harmless.
    Unknown,
    /// Publishes or mutates remote infrastructure.
    Deploy,
    /// Reaches another machine (ssh, tunnels, remote copy, downloads).
    NetworkRemote,
    /// Needs elevated privileges or changes system configuration.
    Sensitive,
    /// Destroys the machine or the user's data; never allowed.
    Catastrophic,
}

impl CommandClass {
    /// Name used in events, rules and the decision table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SafeDev => "safe_dev",
            Self::Unknown => "unknown",
            Self::Deploy => "deploy",
            Self::NetworkRemote => "network_remote",
            Self::Sensitive => "sensitive",
            Self::Catastrophic => "catastrophic",
        }
    }

    /// Parse a label produced by [`CommandClass::label`].
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "safe_dev" => Self::SafeDev,
            "unknown" => Self::Unknown,
            "deploy" => Self::Deploy,
            "network_remote" => Self::NetworkRemote,
            "sensitive" => Self::Sensitive,
            "catastrophic" => Self::Catastrophic,
            _ => return None,
        })
    }

    /// Classes that `auto` mode refuses to run unattended.
    #[must_use]
    pub fn auto_forbidden(self) -> bool {
        matches!(
            self,
            Self::Sensitive | Self::NetworkRemote | Self::Deploy | Self::Catastrophic
        )
    }
}

/// Everything the engine needs to know about a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    /// Most restrictive class of any simple command.
    pub class: CommandClass,
    /// Effective argv of every simple command (wrappers removed, inline
    /// scripts expanded, program name reduced to its lower-case basename).
    /// Used for user rules.
    pub commands: Vec<Vec<String>>,
    /// Words that name filesystem locations: absolute, `~`, `..`, Windows
    /// drive/UNC paths, secret files, and every redirection target.
    pub paths: Vec<PathArg>,
    /// The parser could not follow the whole line.
    pub opaque: bool,
}

/// A filesystem location mentioned by a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathArg {
    pub text: String,
    /// The command writes to it (output redirection).
    pub write: bool,
}

/// Classify a full command line: the most restrictive segment wins.
#[must_use]
pub fn classify(input: &str) -> CommandClass {
    analyze(input).class
}

/// Parse and classify a command line.
#[must_use]
pub fn analyze(input: &str) -> Analysis {
    let mut out = Analysis {
        class: CommandClass::SafeDev,
        commands: Vec::new(),
        paths: Vec::new(),
        opaque: false,
    };
    analyze_into(input, 0, &mut out);
    if has_fork_bomb(input) {
        out.class = CommandClass::Catastrophic;
    }
    out
}

/// Split a command line into simple-command texts (for display and tests).
#[must_use]
pub fn split_segments(input: &str) -> Vec<String> {
    parse(input, 0)
        .commands
        .iter()
        .map(|cmd| {
            cmd.words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| !text.is_empty())
        .collect()
}

const MAX_DEPTH: usize = 12;

fn analyze_into(input: &str, depth: usize, out: &mut Analysis) {
    if depth > MAX_DEPTH {
        raise(&mut out.class, CommandClass::Unknown);
        out.opaque = true;
        return;
    }
    let script = parse(input, depth);
    if script.opaque {
        out.opaque = true;
        raise(&mut out.class, CommandClass::Unknown);
    }
    for cmd in &script.commands {
        let class = classify_command(cmd, &script, depth, out);
        raise(&mut out.class, class);
    }
}

fn raise(current: &mut CommandClass, other: CommandClass) {
    if other > *current {
        *current = other;
    }
}

// ---------------------------------------------------------------------------
// Parser

#[derive(Debug, Clone, Default)]
struct Word {
    text: String,
    /// Contains parameter expansion, command substitution or ANSI-C quoting:
    /// the runtime value is unknown.
    dynamic: bool,
    /// Some part was quoted or escaped (so it cannot be a reserved word or an
    /// assignment).
    quoted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedirectKind {
    Input,
    Output,
    /// `<<` / `<<-`: the target is the delimiter.
    HereDoc {
        strip_tabs: bool,
    },
    HereString,
    /// `>&2`, `<&0`, `>&-`.
    Dup,
}

#[derive(Debug, Clone)]
struct Redirect {
    kind: RedirectKind,
    target: Word,
}

#[derive(Debug, Clone, Default)]
struct SimpleCommand {
    words: Vec<Word>,
    redirects: Vec<Redirect>,
    heredoc_body: Option<String>,
    /// Index of the command whose stdout feeds this one through `|`.
    pipe_from: Option<usize>,
}

#[derive(Debug, Default)]
struct Script {
    commands: Vec<SimpleCommand>,
    opaque: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Eof,
    Paren,
    Backtick,
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    depth: usize,
    script: Script,
}

fn parse(input: &str, depth: usize) -> Script {
    let chars: Vec<char> = input.chars().collect();
    let mut parser = Parser {
        chars: &chars,
        pos: 0,
        depth,
        script: Script::default(),
    };
    parser.parse_list(Stop::Eof);
    parser.script
}

struct Level {
    cur: SimpleCommand,
    word: Option<Word>,
    pending_redirect: Option<RedirectKind>,
    /// Commands at this level with here-documents still to read.
    heredoc_owners: Vec<(usize, String, bool)>,
    pending_heredocs: Vec<(String, bool)>,
    last_index: Option<usize>,
    piped: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    // A single state machine reads best as one function.
    #[allow(clippy::too_many_lines)]
    fn parse_list(&mut self, stop: Stop) {
        if self.depth > MAX_DEPTH {
            self.script.opaque = true;
            self.pos = self.chars.len();
            return;
        }
        let mut level = Level {
            cur: SimpleCommand::default(),
            word: None,
            pending_redirect: None,
            heredoc_owners: Vec::new(),
            pending_heredocs: Vec::new(),
            last_index: None,
            piped: false,
        };
        loop {
            let Some(ch) = self.peek() else {
                if stop != Stop::Eof {
                    self.script.opaque = true;
                }
                self.finish_command(&mut level);
                if !level.heredoc_owners.is_empty() {
                    // Here-document never terminated.
                    self.script.opaque = true;
                }
                return;
            };
            match ch {
                ')' if stop == Stop::Paren => {
                    self.pos += 1;
                    self.finish_command(&mut level);
                    return;
                }
                '`' if stop == Stop::Backtick => {
                    self.pos += 1;
                    self.finish_command(&mut level);
                    return;
                }
                ' ' | '\t' => {
                    self.pos += 1;
                    Self::end_word(&mut level);
                }
                '\n' => {
                    self.pos += 1;
                    self.finish_command(&mut level);
                    level.piped = false;
                    self.read_heredoc_bodies(&mut level);
                }
                '#' if level.word.is_none() => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\n') => self.pos += 1,
                        Some(next) => {
                            self.pos += 1;
                            let word = level.word.get_or_insert_with(Word::default);
                            word.text.push(next);
                            word.quoted = true;
                        }
                        None => {}
                    }
                }
                '\'' => {
                    self.pos += 1;
                    let word = level.word.get_or_insert_with(Word::default);
                    word.quoted = true;
                    let mut closed = false;
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == '\'' {
                            closed = true;
                            break;
                        }
                        word.text.push(c);
                    }
                    if !closed {
                        self.script.opaque = true;
                    }
                }
                '"' => {
                    self.pos += 1;
                    let mut word = level.word.take().unwrap_or_default();
                    word.quoted = true;
                    self.read_double_quoted(&mut word, true);
                    level.word = Some(word);
                }
                '$' => {
                    let mut word = level.word.take().unwrap_or_default();
                    self.read_dollar(&mut word);
                    level.word = Some(word);
                }
                '`' => {
                    self.pos += 1;
                    self.nested(Stop::Backtick);
                    let word = level.word.get_or_insert_with(Word::default);
                    word.text.push_str("`…`");
                    word.dynamic = true;
                }
                '(' => {
                    self.pos += 1;
                    let in_word = level.word.as_ref().is_some_and(|w| !w.text.is_empty());
                    if in_word && self.peek() == Some(')') {
                        // `name()` function definition: the name is not a command.
                        self.pos += 1;
                        level.word = None;
                        self.finish_command(&mut level);
                    } else if in_word {
                        self.script.opaque = true;
                        level.word.get_or_insert_with(Word::default).text.push('(');
                    } else {
                        self.finish_command(&mut level);
                        self.nested(Stop::Paren);
                    }
                }
                ')' => {
                    // Unbalanced close paren.
                    self.pos += 1;
                    self.script.opaque = true;
                    self.finish_command(&mut level);
                }
                '<' | '>' => self.read_redirect(&mut level, ch),
                '&' => {
                    if self.peek_at(1) == Some('>') {
                        // `&>file` / `&>>file`.
                        self.pos += 2;
                        if self.peek() == Some('>') {
                            self.pos += 1;
                        }
                        Self::end_word(&mut level);
                        level.pending_redirect = Some(RedirectKind::Output);
                    } else {
                        self.pos += 1;
                        if self.peek() == Some('&') {
                            self.pos += 1;
                        }
                        self.finish_command(&mut level);
                        level.piped = false;
                    }
                }
                '|' => {
                    self.pos += 1;
                    let piped = match self.peek() {
                        Some('|') => {
                            self.pos += 1;
                            false
                        }
                        Some('&') => {
                            self.pos += 1;
                            true
                        }
                        _ => true,
                    };
                    self.finish_command(&mut level);
                    level.piped = piped;
                }
                ';' => {
                    self.pos += 1;
                    while self.peek() == Some(';') || self.peek() == Some('&') {
                        self.pos += 1;
                    }
                    self.finish_command(&mut level);
                    level.piped = false;
                }
                other => {
                    self.pos += 1;
                    level
                        .word
                        .get_or_insert_with(Word::default)
                        .text
                        .push(other);
                }
            }
        }
    }

    fn nested(&mut self, stop: Stop) {
        self.depth += 1;
        self.parse_list(stop);
        self.depth -= 1;
    }

    fn end_word(level: &mut Level) {
        let Some(word) = level.word.take() else {
            return;
        };
        if let Some(kind) = level.pending_redirect.take() {
            if let RedirectKind::HereDoc { strip_tabs } = kind {
                level.pending_heredocs.push((word.text.clone(), strip_tabs));
            }
            level.cur.redirects.push(Redirect { kind, target: word });
            return;
        }
        level.cur.words.push(word);
    }

    fn finish_command(&mut self, level: &mut Level) {
        Self::end_word(level);
        if level.pending_redirect.take().is_some() {
            self.script.opaque = true;
        }
        let cmd = std::mem::take(&mut level.cur);
        let heredocs = std::mem::take(&mut level.pending_heredocs);
        if cmd.words.is_empty() && cmd.redirects.is_empty() {
            return;
        }
        let mut cmd = cmd;
        if level.piped {
            cmd.pipe_from = level.last_index;
        }
        level.piped = false;
        self.script.commands.push(cmd);
        let index = self.script.commands.len() - 1;
        level.last_index = Some(index);
        for (delimiter, strip_tabs) in heredocs {
            level.heredoc_owners.push((index, delimiter, strip_tabs));
        }
    }

    fn read_heredoc_bodies(&mut self, level: &mut Level) {
        for (index, delimiter, strip_tabs) in std::mem::take(&mut level.heredoc_owners) {
            let mut body = String::new();
            let mut closed = false;
            while self.pos < self.chars.len() {
                let start = self.pos;
                while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                    self.pos += 1;
                }
                let line: String = self.chars[start..self.pos].iter().collect();
                if self.pos < self.chars.len() {
                    self.pos += 1;
                }
                let candidate = if strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if candidate == delimiter {
                    closed = true;
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if !closed {
                self.script.opaque = true;
            }
            // Substitutions in an unquoted body run; scanning them is harmless
            // for a quoted one, so always do it.
            let chars: Vec<char> = body.chars().collect();
            let mut sub = Parser {
                chars: &chars,
                pos: 0,
                depth: self.depth + 1,
                script: Script::default(),
            };
            let mut scratch = Word::default();
            sub.read_double_quoted(&mut scratch, false);
            let offset = self.script.commands.len();
            for mut cmd in sub.script.commands {
                cmd.pipe_from = cmd.pipe_from.map(|i| i + offset);
                self.script.commands.push(cmd);
            }
            self.script.opaque |= sub.script.opaque;
            if let Some(owner) = self.script.commands.get_mut(index) {
                owner.heredoc_body = Some(body);
            }
        }
    }

    fn read_redirect(&mut self, level: &mut Level, first: char) {
        // A word made only of digits right before the operator is an fd number.
        if level.word.as_ref().is_some_and(|w| {
            !w.quoted && !w.text.is_empty() && w.text.chars().all(|c| c.is_ascii_digit())
        }) {
            level.word = None;
        } else {
            Self::end_word(level);
        }
        self.pos += 1;
        let next = self.peek();
        let kind = match (first, next) {
            ('<' | '>', Some('(')) => {
                // Process substitution.
                self.pos += 1;
                self.nested(Stop::Paren);
                let word = level.word.get_or_insert_with(Word::default);
                word.text.push_str("<(…)");
                word.dynamic = true;
                return;
            }
            ('>', Some('>' | '|')) | ('<', Some('>')) => {
                self.pos += 1;
                RedirectKind::Output
            }
            ('>' | '<', Some('&')) => {
                self.pos += 1;
                // `>&2` duplicates; `>&file` writes a file.
                let digits_or_dash = self.peek().is_some_and(|c| c.is_ascii_digit() || c == '-');
                if digits_or_dash {
                    RedirectKind::Dup
                } else if first == '>' {
                    RedirectKind::Output
                } else {
                    RedirectKind::Input
                }
            }
            ('<', Some('<')) => {
                self.pos += 1;
                match self.peek() {
                    Some('<') => {
                        self.pos += 1;
                        RedirectKind::HereString
                    }
                    Some('-') => {
                        self.pos += 1;
                        RedirectKind::HereDoc { strip_tabs: true }
                    }
                    _ => RedirectKind::HereDoc { strip_tabs: false },
                }
            }
            ('>', _) => RedirectKind::Output,
            _ => RedirectKind::Input,
        };
        level.pending_redirect = Some(kind);
        // Skip blanks so the next word becomes the target.
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    /// Read the body of a double-quoted string (opening quote consumed). With
    /// `terminated == false` reads to the end (here-document bodies).
    fn read_double_quoted(&mut self, word: &mut Word, terminated: bool) {
        loop {
            let Some(c) = self.peek() else {
                if terminated {
                    self.script.opaque = true;
                }
                return;
            };
            match c {
                '"' if terminated => {
                    self.pos += 1;
                    return;
                }
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(next @ ('$' | '`' | '"' | '\\')) => {
                            self.pos += 1;
                            word.text.push(next);
                        }
                        Some('\n') => self.pos += 1,
                        Some(next) => {
                            self.pos += 1;
                            word.text.push('\\');
                            word.text.push(next);
                        }
                        None => word.text.push('\\'),
                    }
                }
                '$' => self.read_dollar(word),
                '`' => {
                    self.pos += 1;
                    self.nested(Stop::Backtick);
                    word.text.push_str("`…`");
                    word.dynamic = true;
                }
                other => {
                    self.pos += 1;
                    word.text.push(other);
                }
            }
        }
    }

    /// At a `$`: parameter expansion, command substitution, arithmetic,
    /// ANSI-C or locale quoting.
    // A single state machine reads best as one function.
    #[allow(clippy::too_many_lines)]
    fn read_dollar(&mut self, word: &mut Word) {
        self.pos += 1;
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                self.nested(Stop::Paren);
                word.text.push_str("$(…)");
                word.dynamic = true;
            }
            Some('{') => {
                self.pos += 1;
                word.text.push_str("${");
                word.dynamic = true;
                let mut depth = 1usize;
                loop {
                    let Some(c) = self.peek() else {
                        self.script.opaque = true;
                        return;
                    };
                    match c {
                        '}' => {
                            self.pos += 1;
                            depth -= 1;
                            if depth == 0 {
                                word.text.push('}');
                                return;
                            }
                            word.text.push('}');
                        }
                        '{' => {
                            self.pos += 1;
                            depth += 1;
                            word.text.push('{');
                        }
                        '$' => self.read_dollar(word),
                        '`' => {
                            self.pos += 1;
                            self.nested(Stop::Backtick);
                        }
                        '"' => {
                            self.pos += 1;
                            self.read_double_quoted(word, true);
                        }
                        '\'' => {
                            self.pos += 1;
                            while let Some(q) = self.peek() {
                                self.pos += 1;
                                if q == '\'' {
                                    break;
                                }
                                word.text.push(q);
                            }
                        }
                        '\\' => {
                            self.pos += 1;
                            if let Some(n) = self.peek() {
                                self.pos += 1;
                                word.text.push(n);
                            }
                        }
                        other => {
                            self.pos += 1;
                            word.text.push(other);
                        }
                    }
                }
            }
            Some('\'') => {
                // ANSI-C quoting: escapes can encode anything.
                self.pos += 1;
                word.dynamic = true;
                word.quoted = true;
                word.text.push_str("$'");
                let mut closed = false;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == '\\' {
                        if let Some(n) = self.peek() {
                            self.pos += 1;
                            word.text.push('\\');
                            word.text.push(n);
                        }
                        continue;
                    }
                    if c == '\'' {
                        closed = true;
                        break;
                    }
                    word.text.push(c);
                }
                if !closed {
                    self.script.opaque = true;
                }
            }
            Some('"') => {
                self.pos += 1;
                word.quoted = true;
                self.read_double_quoted(word, true);
            }
            Some(c) if c == '_' || c.is_ascii_alphanumeric() => {
                word.dynamic = true;
                word.text.push('$');
                while let Some(n) = self.peek() {
                    if n == '_' || n.is_ascii_alphanumeric() {
                        self.pos += 1;
                        word.text.push(n);
                    } else {
                        break;
                    }
                }
            }
            Some(c @ ('?' | '#' | '@' | '*' | '!' | '$' | '-')) => {
                self.pos += 1;
                word.dynamic = true;
                word.text.push('$');
                word.text.push(c);
            }
            _ => word.text.push('$'),
        }
    }
}

// ---------------------------------------------------------------------------
// Fork bombs

/// `:(){ :|:& };:` and named variants: a function that pipes into itself in
/// the background.
fn has_fork_bomb(input: &str) -> bool {
    let chars: Vec<char> = input.chars().collect();
    let boundary =
        |c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '{' | '}' | '(' | ')');
    let skip_blank = |mut index: usize| {
        while chars.get(index).is_some_and(|c| *c == ' ' || *c == '\t') {
            index += 1;
        }
        index
    };
    let mut names = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        // `name ( ) {` — POSIX function definition.
        if *ch == '(' {
            let close = skip_blank(index + 1);
            if chars.get(close) == Some(&')') && chars.get(skip_blank(close + 1)) == Some(&'{') {
                let mut start = index;
                while start > 0 && chars[start - 1] == ' ' {
                    start -= 1;
                }
                let end = start;
                while start > 0 && !boundary(chars[start - 1]) {
                    start -= 1;
                }
                if start < end {
                    names.push(chars[start..end].iter().collect::<String>());
                }
            }
        }
    }
    // `function name {` — ksh/bash keyword form.
    let words: Vec<&str> = input
        .split(|c: char| c.is_whitespace() || c == ';')
        .filter(|w| !w.is_empty())
        .collect();
    for pair in words.windows(2) {
        if pair[0] == "function" {
            let name = pair[1].trim_end_matches(['{', '(', ')']);
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    let compact: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    names.iter().any(|name| {
        compact.contains(&format!("{name}|{name}&")) || compact.contains(&format!("{name}|{name};"))
    })
}

// ---------------------------------------------------------------------------
// Classification tables

/// Programs that only read. Dangerous flags are handled in `is_safe`.
const READ_ONLY: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "rg", "grep", "fd", "find", "tree", "file", "stat", "which",
    "echo", "printf", "pwd", "true", "false", "sort", "uniq", "diff", "basename", "dirname",
    "realpath", "test", "[",
];

/// Read-only programs whose operands are filesystem paths.
const PATH_OPERAND_PROGRAMS: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "rg", "grep", "fd", "find", "tree", "file", "stat", "sort",
    "uniq", "diff", "realpath", "cd", "pushd",
];

/// Tools that are safe whatever their arguments (project-local effects only).
const SAFE_TOOLS: &[&str] = &[
    "tsc", "vitest", "jest", "pytest", "eslint", "prettier", "ruff", "mypy",
];

/// Package managers and build tools: the subcommand must come first.
const SAFE_SUBCOMMANDS: &[(&str, &[&str])] = &[
    (
        "pnpm",
        &[
            "test",
            "t",
            "lint",
            "build",
            "run",
            "install",
            "i",
            "fmt",
            "check",
            "typecheck",
        ],
    ),
    (
        "npm",
        &["test", "t", "run", "run-script", "install", "i", "ci", "ls"],
    ),
    ("yarn", &["test", "run", "install", "build", "lint"]),
    ("bun", &["test", "run", "install", "i", "build"]),
    (
        "cargo",
        &[
            "build", "b", "test", "t", "check", "c", "clippy", "fmt", "run", "r", "doc", "bench",
            "metadata", "tree", "nextest",
        ],
    ),
    (
        "git",
        &[
            "status",
            "diff",
            "log",
            "show",
            "add",
            "commit",
            "rev-parse",
            "blame",
            "ls-files",
            "describe",
        ],
    ),
    ("uv", &["sync", "lock"]),
    ("go", &["build", "test", "vet"]),
];

/// `npm install` etc. are safe only from the lockfile, not with a package.
const INSTALL_VERBS: &[&str] = &["install", "i", "ci", "add"];

/// Python modules that are test/lint runners.
const SAFE_PYTHON_MODULES: &[&str] = &["pytest", "unittest", "mypy", "ruff", "black"];

/// Environment assignments that cannot change which code runs.
const SAFE_ENV: &[&str] = &[
    "CI",
    "NODE_ENV",
    "RUST_BACKTRACE",
    "RUST_LOG",
    "DEBUG",
    "FORCE_COLOR",
    "NO_COLOR",
    "TZ",
    "LANG",
    "LC_ALL",
    "TERM",
    "COLUMNS",
];

const SENSITIVE_PROGRAMS: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "pkexec",
    "run0",
    "apt",
    "apt-get",
    "dpkg",
    "dnf",
    "yum",
    "rpm",
    "pacman",
    "zypper",
    "apk",
    "emerge",
    "snap",
    "flatpak",
    "brew",
    "port",
    "mas",
    "softwareupdate",
    "choco",
    "winget",
    "scoop",
    "systemctl",
    "service",
    "launchctl",
    "crontab",
    "at",
    "defaults",
    "reg",
    "regedit",
    "sc",
    "schtasks",
    "bcdedit",
    "icacls",
    "takeown",
    "setx",
    "wmic",
    "vssadmin",
    "net",
    "netsh",
    "iptables",
    "ip6tables",
    "ufw",
    "firewall-cmd",
    "pfctl",
    "nft",
    "diskutil",
    "security",
    "csrutil",
    "spctl",
    "tccutil",
    "nvram",
    "bless",
    "kextload",
    "kextunload",
    "kmutil",
    "dscl",
    "sysadminctl",
    "passwd",
    "chpasswd",
    "visudo",
    "useradd",
    "usermod",
    "userdel",
    "groupadd",
    "mount",
    "umount",
    "modprobe",
    "insmod",
    "rmmod",
    "sysctl",
    "setenforce",
    "chattr",
    "xattr",
    "osascript",
    "chmod",
    "chown",
    "chgrp",
    "killall",
    "pkill",
];

const NETWORK_PROGRAMS: &[&str] = &[
    "ssh",
    "scp",
    "sftp",
    "ssh-copy-id",
    "sshpass",
    "autossh",
    "mosh",
    "telnet",
    "nc",
    "netcat",
    "ncat",
    "socat",
    "ngrok",
    "cloudflared",
    "tailscale",
    "frpc",
    "bore",
    "zrok",
    "lt",
    "rclone",
    "ftp",
    "tftp",
    "curl",
    "wget",
    "aria2c",
    "http",
    "https",
    "xh",
    "invoke-webrequest",
    "iwr",
    "invoke-restmethod",
    "irm",
    "bitsadmin",
    "certutil",
];

/// Deploy verbs, matched as `program … verb`.
const DEPLOY_PAIRS: &[(&str, &[&str])] = &[
    ("git", &["push"]),
    ("vercel", &["deploy", "--prod", "promote"]),
    ("fly", &["deploy", "launch"]),
    ("flyctl", &["deploy", "launch"]),
    (
        "kubectl",
        &[
            "apply", "create", "delete", "patch", "replace", "rollout", "scale", "drain", "set",
            "edit",
        ],
    ),
    ("helm", &["upgrade", "install", "uninstall", "rollback"]),
    ("terraform", &["apply", "destroy", "import"]),
    ("tofu", &["apply", "destroy", "import"]),
    ("pulumi", &["up", "destroy"]),
    ("netlify", &["deploy"]),
    ("heroku", &["deploy", "pg:push", "releases:rollback"]),
    ("aws", &["s3", "cloudformation", "ecs", "lambda", "deploy"]),
    ("gcloud", &["deploy", "app", "run"]),
    ("firebase", &["deploy"]),
    ("wrangler", &["deploy", "publish"]),
    ("serverless", &["deploy"]),
    ("sls", &["deploy"]),
    ("railway", &["up"]),
    ("docker", &["push"]),
    ("podman", &["push"]),
    ("npm", &["publish", "unpublish"]),
    ("pnpm", &["publish"]),
    ("yarn", &["publish"]),
    ("bun", &["publish"]),
    ("cargo", &["publish", "yank"]),
    ("twine", &["upload"]),
    ("gh", &["release", "merge"]),
];

/// Programs that always deploy.
const DEPLOY_PROGRAMS: &[&str] = &["ansible-playbook"];

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "mksh", "fish", "ash", "yash", "tcsh", "csh",
];

const BLOCK_DEVICE_PREFIXES: &[&str] = &[
    "/dev/sd",
    "/dev/hd",
    "/dev/vd",
    "/dev/xvd",
    "/dev/nvme",
    "/dev/disk",
    "/dev/rdisk",
    "/dev/mmcblk",
    "/dev/mapper",
    "/dev/md",
    "/dev/dm-",
];

const HARMLESS_DEVICES: &[&str] = &[
    "/dev/null",
    "/dev/stdin",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
];

// ---------------------------------------------------------------------------
// Simple-command classification

/// Lower-case basename without a Windows executable extension. macOS and
/// Windows resolve program names case-insensitively, so `RM` is `rm`.
fn program_name(word: &str) -> String {
    let base = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .to_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stem) = base.strip_suffix(ext) {
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }
    base
}

/// Explicit program paths are trusted only in system binary directories;
/// `./ls` or `/tmp/x/cat` may be anything.
fn trusted_program_path(word: &str) -> bool {
    if !word.contains(['/', '\\']) {
        return true;
    }
    [
        "/bin/",
        "/usr/bin/",
        "/usr/local/bin/",
        "/sbin/",
        "/usr/sbin/",
        "/opt/homebrew/bin/",
    ]
    .iter()
    .any(|dir| {
        word.strip_prefix(dir)
            .is_some_and(|rest| !rest.is_empty() && !rest.contains(['/', '\\']))
    })
}

fn is_assignment(word: &Word) -> Option<&str> {
    let (name, _) = word.text.split_once('=')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric()))
    .then_some(name)
}

fn at(words: &[Word], i: usize, offset: usize) -> Option<&str> {
    words.get(i + offset).map(|w| w.text.as_str())
}

fn is_python(program: &str) -> bool {
    program
        .strip_prefix("python")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

#[allow(clippy::too_many_lines)]
fn classify_command(
    cmd: &SimpleCommand,
    script: &Script,
    depth: usize,
    out: &mut Analysis,
) -> CommandClass {
    let mut class = CommandClass::SafeDev;

    collect_redirect_paths(cmd, &mut class, out);

    let words = &cmd.words;
    let mut i = 0;
    // Leading assignments: `A=1 cmd`.
    while i < words.len() {
        let Some(name) = is_assignment(&words[i]) else {
            break;
        };
        if !SAFE_ENV.contains(&name) {
            raise(&mut class, CommandClass::Unknown);
        }
        collect_word_path(&words[i], out);
        i += 1;
    }
    // Reserved words.
    while i < words.len() && !words[i].quoted {
        match words[i].text.as_str() {
            "!" | "{" | "}" | "if" | "then" | "else" | "elif" | "fi" | "do" | "done" | "while"
            | "until" | "esac" => i += 1,
            "for" | "select" | "case" | "function" | "in" | "[[" => {
                for word in &words[i..] {
                    collect_word_path(word, out);
                    if word.dynamic {
                        raise(&mut class, CommandClass::Unknown);
                    }
                }
                return class;
            }
            _ => break,
        }
    }
    if i >= words.len() {
        return class;
    }

    for word in &words[i..] {
        collect_word_path(word, out);
    }

    // Wrappers.
    loop {
        if i >= words.len() {
            return class;
        }
        if words[i].dynamic {
            // The program itself is computed at runtime.
            raise(&mut class, CommandClass::Unknown);
            out.commands
                .push(words[i..].iter().map(|w| w.text.clone()).collect());
            return class;
        }
        let program = program_name(&words[i].text);
        match program.as_str() {
            "sudo" | "doas" | "pkexec" | "run0" => {
                raise(&mut class, CommandClass::Sensitive);
                i += 1;
                while let Some(flag) = words.get(i).map(|w| w.text.as_str()) {
                    if flag == "--" {
                        i += 1;
                        break;
                    }
                    if !flag.starts_with('-') || flag == "-" {
                        break;
                    }
                    let takes_value = matches!(
                        flag,
                        "-u" | "-g"
                            | "-p"
                            | "-U"
                            | "-C"
                            | "-h"
                            | "-r"
                            | "-t"
                            | "-D"
                            | "-R"
                            | "-T"
                            | "--user"
                            | "--group"
                            | "--prompt"
                            | "--chdir"
                            | "--host"
                    );
                    i += if takes_value { 2 } else { 1 };
                }
            }
            "su" => {
                raise(&mut class, CommandClass::Sensitive);
                let rest = &words[i + 1..];
                if let Some(at) = rest
                    .iter()
                    .position(|w| w.text == "-c" || w.text == "--command")
                {
                    if let Some(script_word) = rest.get(at + 1) {
                        analyze_into(&script_word.text, depth + 1, out);
                    }
                }
                return class;
            }
            "env" => {
                i += 1;
                loop {
                    let Some(word) = words.get(i) else {
                        // Bare `env` prints the environment (tokens, keys).
                        raise(&mut class, CommandClass::Unknown);
                        return class;
                    };
                    let text = word.text.as_str();
                    if text == "--" {
                        i += 1;
                        break;
                    } else if matches!(
                        text,
                        "-i" | "-0" | "-" | "-v" | "--ignore-environment" | "--null" | "--debug"
                    ) {
                        i += 1;
                    } else if matches!(text, "-u" | "--unset") {
                        i += 2;
                    } else if matches!(text, "-C" | "--chdir") {
                        raise(&mut class, CommandClass::Unknown);
                        i += 2;
                    } else if text == "-S" || text == "--split-string" {
                        raise(&mut class, CommandClass::Unknown);
                        let joined = words[i + 1..]
                            .iter()
                            .map(|w| w.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        analyze_into(&joined, depth + 1, out);
                        return class;
                    } else if let Some(rest) = text.strip_prefix("-S") {
                        raise(&mut class, CommandClass::Unknown);
                        let mut joined = rest.to_string();
                        for w in &words[i + 1..] {
                            joined.push(' ');
                            joined.push_str(&w.text);
                        }
                        analyze_into(&joined, depth + 1, out);
                        return class;
                    } else if text.starts_with('-') {
                        raise(&mut class, CommandClass::Unknown);
                        i += 1;
                    } else if let Some(name) = is_assignment(word) {
                        if !SAFE_ENV.contains(&name) {
                            raise(&mut class, CommandClass::Unknown);
                        }
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            "command" => {
                if matches!(at(words, i, 1), Some("-v" | "-V")) {
                    return class;
                }
                i += 1;
                while at(words, i, 0).is_some_and(|a| a == "-p") {
                    i += 1;
                }
            }
            "builtin" | "nohup" | "unbuffer" | "chronic" | "busybox" | "toybox" => i += 1,
            "exec" => {
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    match flag {
                        "-a" => i += 2,
                        "-c" | "-l" => i += 1,
                        _ => break,
                    }
                }
            }
            "time" => {
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    match flag {
                        "-o" | "--output" => {
                            raise(&mut class, CommandClass::Unknown);
                            i += 2;
                        }
                        f if f.starts_with('-') => i += 1,
                        _ => break,
                    }
                }
            }
            "nice" | "ionice" | "caffeinate" | "stdbuf" => {
                let value_flags: &[&str] = match program.as_str() {
                    "nice" => &["-n"],
                    "ionice" => &["-c", "-n", "-p"],
                    "caffeinate" => &["-t", "-w"],
                    _ => &["-i", "-o", "-e"],
                };
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    if !flag.starts_with('-') {
                        break;
                    }
                    i += if value_flags.contains(&flag) { 2 } else { 1 };
                }
            }
            "timeout" | "gtimeout" => {
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    if !flag.starts_with('-') {
                        break;
                    }
                    i += if matches!(flag, "-s" | "-k" | "--signal" | "--kill-after") {
                        2
                    } else {
                        1
                    };
                }
                i += 1; // duration
            }
            "xargs" => {
                raise(&mut class, CommandClass::Unknown);
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    if !flag.starts_with('-') {
                        break;
                    }
                    let takes_value = matches!(
                        flag,
                        "-n" | "-L" | "-P" | "-I" | "-d" | "-E" | "-e" | "-s" | "-a" | "-R" | "-S"
                    );
                    i += if takes_value { 2 } else { 1 };
                }
                if i >= words.len() {
                    // Default command is `echo`.
                    return class;
                }
            }
            "watch" => {
                raise(&mut class, CommandClass::Unknown);
                i += 1;
                while let Some(flag) = at(words, i, 0) {
                    if !flag.starts_with('-') {
                        break;
                    }
                    i += if matches!(flag, "-n" | "--interval") {
                        2
                    } else {
                        1
                    };
                }
                let joined = words[i.min(words.len())..]
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                analyze_into(&joined, depth + 1, out);
                return class;
            }
            "uv" if at(words, i, 1) == Some("run") => {
                i += 2;
                if at(words, i, 0).is_some_and(|a| a.starts_with('-')) {
                    raise(&mut class, CommandClass::Unknown);
                    return class;
                }
            }
            "pnpm" | "yarn" | "npm" if at(words, i, 1) == Some("exec") => {
                i += 2;
                if at(words, i, 0) == Some("--") {
                    i += 1;
                } else if at(words, i, 0).is_some_and(|a| a.starts_with('-')) {
                    raise(&mut class, CommandClass::Unknown);
                    return class;
                }
            }
            "npx" | "pnpx" | "bunx" | "uvx" => {
                // May download and run a package that is not in the project.
                raise(&mut class, CommandClass::Unknown);
                i += 1;
                while at(words, i, 0).is_some_and(|a| a.starts_with('-')) {
                    i += 1;
                }
            }
            "pnpm" | "bun" if matches!(at(words, i, 1), Some("dlx" | "x")) => {
                raise(&mut class, CommandClass::Unknown);
                i += 2;
                while at(words, i, 0).is_some_and(|a| a.starts_with('-')) {
                    i += 1;
                }
            }
            _ => break,
        }
    }
    if i >= words.len() {
        return class;
    }

    let head = &words[i];
    let program = program_name(&head.text);
    let args: Vec<&str> = words[i + 1..].iter().map(|w| w.text.as_str()).collect();
    let mut effective = vec![program.clone()];
    effective.extend(args.iter().map(|a| (*a).to_string()));
    out.commands.push(effective);

    // Programs whose operands are files: check every operand, not only the
    // ones that look like paths, so a symlinked directory inside the
    // workspace cannot smuggle `keys/id_rsa` past the boundary check.
    if PATH_OPERAND_PROGRAMS.contains(&program.as_str()) {
        for word in &words[i + 1..] {
            if !word.dynamic && !word.text.starts_with('-') && !word.text.is_empty() {
                out.paths.push(PathArg {
                    text: word.text.clone(),
                    write: false,
                });
            }
        }
    }
    if !trusted_program_path(&head.text) {
        raise(&mut class, CommandClass::Unknown);
    }
    if words[i + 1..].iter().any(|w| w.dynamic) {
        raise(&mut class, CommandClass::Unknown);
    }

    let specific = classify_program(&program, &args, cmd, script, depth, out);
    raise(&mut class, specific);
    class
}

fn classify_program(
    program: &str,
    args: &[&str],
    cmd: &SimpleCommand,
    script: &Script,
    depth: usize,
    out: &mut Analysis,
) -> CommandClass {
    if is_catastrophic(program, args) {
        return CommandClass::Catastrophic;
    }
    if let Some(class) = inline_script(program, args, cmd, script, depth, out) {
        return class;
    }
    if program == "systemctl"
        && args
            .iter()
            .any(|a| matches!(*a, "poweroff" | "reboot" | "halt" | "kexec"))
    {
        return CommandClass::Catastrophic;
    }
    if program == "cd" || program == "pushd" {
        // Bare `cd`/`cd -` jump to `$HOME`/`$OLDPWD`, which relative paths
        // later in the line would then silently address.
        let targets = operands(args);
        return if targets.len() == 1 && targets[0] != "-" {
            CommandClass::SafeDev
        } else {
            CommandClass::Unknown
        };
    }
    if SENSITIVE_PROGRAMS.contains(&program) {
        return CommandClass::Sensitive;
    }
    if NETWORK_PROGRAMS.contains(&program) {
        return CommandClass::NetworkRemote;
    }
    if program == "rsync"
        && args
            .iter()
            .any(|a| !a.starts_with('-') && (a.contains(':') || a.starts_with("rsync://")))
    {
        return CommandClass::NetworkRemote;
    }
    if matches!(program, "npm" | "pnpm" | "yarn" | "bun")
        && (args
            .iter()
            .any(|a| matches!(*a, "-g" | "--global" | "--location=global"))
            || args.first() == Some(&"global"))
    {
        return CommandClass::Sensitive;
    }
    if is_deploy(program, args) {
        return CommandClass::Deploy;
    }
    if is_safe(program, args) {
        CommandClass::SafeDev
    } else {
        CommandClass::Unknown
    }
}

/// Inline scripts: shells with `-c`, interpreters with `-c`/`-e`, `eval`.
/// Floor `unknown`; the inner text is classified as shell where possible.
#[allow(clippy::too_many_lines)]
fn inline_script(
    program: &str,
    args: &[&str],
    cmd: &SimpleCommand,
    script: &Script,
    depth: usize,
    out: &mut Analysis,
) -> Option<CommandClass> {
    let inner = |text: &str, out: &mut Analysis| {
        let mut sub = Analysis {
            class: CommandClass::SafeDev,
            commands: Vec::new(),
            paths: Vec::new(),
            opaque: false,
        };
        analyze_into(text, depth + 1, &mut sub);
        if has_fork_bomb(text) {
            sub.class = CommandClass::Catastrophic;
        }
        out.commands.extend(sub.commands);
        out.paths.extend(sub.paths);
        out.opaque |= sub.opaque;
        sub.class.max(CommandClass::Unknown)
    };

    if SHELLS.contains(&program) {
        let mut has_c = false;
        let mut idx = 0;
        while idx < args.len() {
            let a = args[idx];
            if a == "--" || a == "-" {
                idx += 1;
                break;
            }
            if matches!(a, "-o" | "+o" | "-O" | "+O" | "--rcfile" | "--init-file") {
                idx += 2;
                continue;
            }
            if a.starts_with("--") {
                idx += 1;
                continue;
            }
            if (a.starts_with('-') || a.starts_with('+')) && a.len() > 1 {
                if a.starts_with('-') && a[1..].contains('c') {
                    has_c = true;
                }
                idx += 1;
                continue;
            }
            break;
        }
        if has_c {
            return Some(match args.get(idx) {
                Some(text) => inner(text, out),
                None => CommandClass::Unknown,
            });
        }
        if idx < args.len() {
            // Runs a script file.
            return Some(CommandClass::Unknown);
        }
        // Reads its script from stdin.
        if let Some(text) = stdin_script(cmd, script) {
            return Some(inner(&text, out));
        }
        return Some(CommandClass::Unknown);
    }

    if program == "cmd" {
        if let Some(at) = args
            .iter()
            .position(|a| matches!(a.to_ascii_lowercase().as_str(), "/c" | "/k" | "/r"))
        {
            return Some(inner(&args[at + 1..].join(" "), out));
        }
        return Some(CommandClass::Unknown);
    }

    if program == "powershell" || program == "pwsh" {
        for (at, a) in args.iter().enumerate() {
            let lower = a.to_ascii_lowercase();
            if lower.len() >= 2 && "-command".starts_with(&lower) {
                return Some(inner(&args[at + 1..].join(" "), out));
            }
            if lower.len() >= 2
                && ("-encodedcommand".starts_with(&lower) || lower == "-ec" || lower == "-enc")
            {
                return Some(CommandClass::Unknown);
            }
        }
        return Some(CommandClass::Unknown);
    }

    if program == "eval" {
        return Some(inner(&args.join(" "), out));
    }
    if program == "source" || program == "." {
        return Some(CommandClass::Unknown);
    }

    let inline_flags: &[&str] = if is_python(program) {
        &["c"]
    } else {
        match program {
            "node" | "nodejs" => &["e", "p", "-eval", "-print"],
            "perl" | "ruby" => &["e", "E"],
            "php" => &["r"],
            "lua" | "luajit" | "rscript" => &["e"],
            "bun" => &["e", "-eval", "p", "-print"],
            "deno" => {
                return matches!(args.first(), Some(&"eval")).then(|| {
                    let text = args[1..].join(" ");
                    inner(&text, out)
                });
            }
            _ => return None,
        }
    };
    for (at, a) in args.iter().enumerate() {
        let Some(body) = a.strip_prefix('-') else {
            break;
        };
        let long = body.starts_with('-');
        let matched = if long {
            let name = body.split('=').next().unwrap_or(body);
            inline_flags.contains(&name)
        } else {
            body.chars().any(|c| {
                inline_flags
                    .iter()
                    .any(|f| f.len() == 1 && f.starts_with(c))
            })
        };
        if matched {
            let text = if long {
                body.split_once('=')
                    .map(|(_, v)| v.to_string())
                    .or_else(|| args.get(at + 1).map(|s| (*s).to_string()))
            } else {
                args.get(at + 1).map(|s| (*s).to_string())
            };
            return Some(match text {
                Some(text) => inner(&text, out),
                None => CommandClass::Unknown,
            });
        }
    }
    None
}

/// Script text a stdin-reading shell will run: here-string, here-document
/// body, or the literal argument of an `echo`/`printf` piped into it.
fn stdin_script(cmd: &SimpleCommand, script: &Script) -> Option<String> {
    for redirect in &cmd.redirects {
        if redirect.kind == RedirectKind::HereString {
            return Some(redirect.target.text.clone());
        }
    }
    if let Some(body) = &cmd.heredoc_body {
        return Some(body.clone());
    }
    let source = script.commands.get(cmd.pipe_from?)?;
    if let Some(body) = &source.heredoc_body {
        return Some(body.clone());
    }
    let first = source.words.first()?;
    let program = program_name(&first.text);
    if program == "echo" || program == "printf" {
        let text = source.words[1..]
            .iter()
            .filter(|w| !w.text.starts_with('-'))
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return Some(text.replace("\\n", "\n"));
    }
    None
}

fn is_deploy(program: &str, args: &[&str]) -> bool {
    if DEPLOY_PROGRAMS.contains(&program) {
        return true;
    }
    if args
        .iter()
        .any(|arg| *arg == "--prod" || *arg == "--production")
    {
        return true;
    }
    DEPLOY_PAIRS
        .iter()
        .any(|(name, verbs)| *name == program && args.iter().any(|arg| verbs.contains(arg)))
}

fn is_safe(program: &str, args: &[&str]) -> bool {
    if READ_ONLY.contains(&program) {
        let blocked: &[&str] = match program {
            "find" => &[
                "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf",
                "-fls",
            ],
            "fd" => &["-x", "--exec", "-X", "--exec-batch"],
            "rg" => &["--pre"],
            "tree" => &["-o"],
            "file" => &["-C", "--compile"],
            "sort" => &["-o", "--output"],
            _ => &[],
        };
        return !args.iter().any(|arg| {
            blocked.contains(arg)
                || blocked
                    .iter()
                    .any(|b| b.starts_with("--") && arg.starts_with(&format!("{b}=")))
                || (program == "sort" && arg.starts_with("-o"))
        });
    }
    if SAFE_TOOLS.contains(&program) {
        return true;
    }
    if is_python(program) {
        return args.first() == Some(&"-m")
            && args.get(1).is_some_and(|m| SAFE_PYTHON_MODULES.contains(m));
    }
    if program == "node" {
        return args.first() == Some(&"--test");
    }
    let Some((_, verbs)) = SAFE_SUBCOMMANDS.iter().find(|(name, _)| *name == program) else {
        return false;
    };
    let Some(first) = args.first() else {
        // `cargo`, `git` alone print help; `yarn`/`pnpm`/`bun` alone install.
        return true;
    };
    if !verbs.contains(first) {
        return false;
    }
    if program == "git"
        && args
            .iter()
            .any(|a| a.starts_with("--output") || *a == "--ext-diff")
    {
        return false;
    }
    if INSTALL_VERBS.contains(first) {
        // From the lockfile only; a named package is new code from the network.
        return args[1..].iter().all(|a| a.starts_with('-'));
    }
    true
}

/// rm/chmod targets that wipe the system or the user's home.
fn is_root_target(arg: &str) -> bool {
    let mut text = arg.replace('\\', "/");
    for home in [
        "$HOME",
        "${HOME}",
        "$USERPROFILE",
        "${USERPROFILE}",
        "%USERPROFILE%",
        "%HOMEPATH%",
    ] {
        if let Some(rest) = text.strip_prefix(home) {
            text = format!("~{rest}");
        }
    }
    while text.contains("//") {
        text = text.replace("//", "/");
    }
    while text.contains("/./") {
        text = text.replace("/./", "/");
    }
    loop {
        if text.len() > 2 && (text.ends_with("/*") || text.ends_with("/.")) {
            text.truncate(text.len() - 2);
        } else if text.len() > 1 && text.ends_with('/') {
            text.pop();
        } else {
            break;
        }
    }
    if matches!(text.as_str(), "/" | "/*" | "/." | "~" | "." | "..") {
        return true;
    }
    // `$VAR`, `${VAR}` (optionally `/` or `/*` after it): an unset variable
    // turns this into `/`.
    if text.starts_with('$') && !text.contains('/') {
        return true;
    }
    // Windows drive roots and system folders.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rest = lower[2..].trim_start_matches('/');
        return matches!(
            rest,
            "" | "*" | "windows" | "users" | "program files" | "program files (x86)"
        );
    }
    // Top-level directories (`/usr`, `/etc`, `/home`) and home directories.
    if let Some(rest) = text.strip_prefix('/') {
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        return parts.len() <= 1
            || (parts.len() == 2 && matches!(parts[0], "home" | "Users" | "users"));
    }
    // `~/x` is somebody's data but not the whole home.
    false
}

fn operands<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut options_done = false;
    for arg in args {
        if !options_done && *arg == "--" {
            options_done = true;
            continue;
        }
        if !options_done && arg.starts_with('-') && arg.len() > 1 {
            continue;
        }
        out.push(*arg);
    }
    out
}

fn has_recursive_flag(args: &[&str]) -> bool {
    args.iter().take_while(|a| **a != "--").any(|arg| {
        *arg == "--recursive"
            || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(['r', 'R']))
    })
}

fn is_catastrophic(program: &str, args: &[&str]) -> bool {
    if program.starts_with("mkfs")
        || program.starts_with("newfs")
        || matches!(
            program,
            "mke2fs"
                | "mkswap"
                | "wipefs"
                | "fdisk"
                | "sfdisk"
                | "gdisk"
                | "sgdisk"
                | "parted"
                | "diskpart"
                | "format"
                | "format-volume"
                | "clear-disk"
                | "shutdown"
                | "reboot"
                | "halt"
                | "poweroff"
                | "stop-computer"
                | "restart-computer"
        )
    {
        return true;
    }
    let no_preserve = args.iter().any(|a| a.starts_with("--no-preserve-root"));
    match program {
        "rm" => {
            let targets = operands(args);
            (has_recursive_flag(args) && targets.iter().any(|t| is_root_target(t)))
                || (no_preserve && !targets.is_empty())
        }
        "rmdir" | "rd" | "del" | "erase" => {
            let recursive = args.iter().any(|a| a.eq_ignore_ascii_case("/s"));
            recursive
                && args
                    .iter()
                    .filter(|a| !a.starts_with('/') || a.len() > 2)
                    .any(|t| is_root_target(t))
        }
        "remove-item" | "ri" => {
            let recursive = args
                .iter()
                .any(|a| a.len() > 2 && "-recurse".starts_with(&a.to_ascii_lowercase()));
            recursive
                && args
                    .iter()
                    .filter(|a| !a.starts_with('-'))
                    .any(|t| is_root_target(t))
        }
        "chmod" | "chown" | "chgrp" => {
            let targets = operands(args);
            has_recursive_flag(args) && targets.iter().skip(1).any(|t| is_root_target(t))
        }
        "mv" => {
            let targets = operands(args);
            targets.len() >= 2
                && targets[..targets.len() - 1]
                    .iter()
                    .any(|t| is_root_target(t) && *t != "." && *t != "*")
        }
        "find" => {
            let destructive = args
                .iter()
                .any(|a| *a == "-delete" || *a == "-exec" || *a == "-execdir");
            // `find . -name '*.log' -delete` is routine cleanup; only system or
            // home roots are catastrophic here.
            destructive
                && args
                    .iter()
                    .take_while(|a| !a.starts_with('-'))
                    .any(|t| is_root_target(t) && !matches!(*t, "." | ".." | "./" | "../"))
        }
        "dd" => args.iter().any(|arg| {
            arg.strip_prefix("of=")
                .is_some_and(|target| is_block_device(target) || target.starts_with("\\\\.\\"))
        }),
        "shred" => args.iter().any(|a| is_block_device(a)),
        "diskutil" => args.iter().any(|a| {
            let lower = a.to_ascii_lowercase();
            lower.starts_with("erase")
                || lower.starts_with("zero")
                || lower.starts_with("random")
                || lower == "secureerase"
                || lower == "partitiondisk"
                || lower == "reformat"
                || lower == "deletecontainer"
                || lower == "deletevolume"
        }),
        // `kill -9 -1`: every process the user owns.
        "kill" => args.len() >= 2 && args.last() == Some(&"-1"),
        "init" | "telinit" => matches!(args.first(), Some(&("0" | "6"))),
        "vssadmin" => args.iter().any(|a| a.eq_ignore_ascii_case("delete")),
        _ => false,
    }
}

fn is_block_device(target: &str) -> bool {
    BLOCK_DEVICE_PREFIXES
        .iter()
        .any(|prefix| target.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// Path arguments

fn collect_redirect_paths(cmd: &SimpleCommand, class: &mut CommandClass, out: &mut Analysis) {
    for redirect in &cmd.redirects {
        let target = &redirect.target;
        match redirect.kind {
            RedirectKind::Dup | RedirectKind::HereDoc { .. } | RedirectKind::HereString => {}
            RedirectKind::Input | RedirectKind::Output => {
                let write = redirect.kind == RedirectKind::Output;
                if write && is_block_device(&target.text) {
                    raise(class, CommandClass::Catastrophic);
                }
                if target.dynamic {
                    raise(class, CommandClass::Unknown);
                    continue;
                }
                if HARMLESS_DEVICES.contains(&target.text.as_str())
                    || target.text.starts_with("/dev/fd/")
                {
                    continue;
                }
                out.paths.push(PathArg {
                    text: target.text.clone(),
                    write,
                });
            }
        }
    }
}

/// Whether a shell word names a location worth checking.
fn looks_like_path(text: &str) -> bool {
    if text.is_empty() || HARMLESS_DEVICES.contains(&text) || text.starts_with("/dev/fd/") {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    let windows_abs = chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':';
    let name = text.rsplit(['/', '\\']).next().unwrap_or(text);
    text.starts_with('/')
        || text.starts_with('~')
        || text.starts_with("\\\\")
        || windows_abs
        || text == ".."
        || text.starts_with("../")
        || text.starts_with("..\\")
        || text.contains("/../")
        || text.contains("\\..\\")
        || text.ends_with("/..")
        || text.ends_with("\\..")
        || crate::path::is_secret_name(name)
        || text.split(['/', '\\']).any(|part| {
            crate::path::SENSITIVE_ANYWHERE
                .iter()
                .any(|s| s.eq_ignore_ascii_case(part))
        })
}

fn collect_word_path(word: &Word, out: &mut Analysis) {
    if word.dynamic {
        return;
    }
    let text = word.text.as_str();
    let candidate = if text.starts_with('-') {
        // `--output=/etc/x`, `-o/etc/x` is too ambiguous to split.
        match text.split_once('=') {
            Some((_, value)) => value,
            None => return,
        }
    } else if let Some((name, value)) = text.split_once('=') {
        // `VAR=/path` assignments and `if=/dev/sda`-style operands.
        if name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
            value
        } else {
            text
        }
    } else {
        text
    };
    if looks_like_path(candidate) {
        out.paths.push(PathArg {
            text: candidate.to_string(),
            write: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CommandClass::{Catastrophic, Deploy, NetworkRemote, SafeDev, Sensitive, Unknown};

    #[test]
    fn splits_compound_commands() {
        assert_eq!(
            split_segments("a && b || c; d | e"),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(split_segments("echo 'a | b'"), ["echo a | b"]);
        assert_eq!(
            split_segments("echo $(rm -rf /)"),
            ["rm -rf /", "echo $(…)"]
        );
        assert_eq!(split_segments("echo `whoami`"), ["whoami", "echo `…`"]);
        assert_eq!(split_segments("(cd x; ls)"), ["cd x", "ls"]);
        assert_eq!(
            split_segments("cargo test 2>&1 | tail"),
            ["cargo test", "tail"]
        );
    }

    #[test]
    fn worst_segment_wins() {
        assert_eq!(classify("ls && rm -rf /"), Catastrophic);
        assert_eq!(classify("pnpm test; ssh host"), NetworkRemote);
        assert_eq!(classify("cargo test | tee out.txt"), Unknown);
    }

    #[test]
    fn substitutions_in_double_quotes_are_seen() {
        assert_eq!(classify("echo \"$(rm -rf /)\""), Catastrophic);
        assert_eq!(classify("echo \"`rm -rf ~`\""), Catastrophic);
        assert_eq!(classify("echo ${X:-$(rm -rf /)}"), Catastrophic);
        assert_eq!(classify("echo 'not $(rm -rf /)'"), SafeDev);
    }

    #[test]
    fn redirections_are_paths_not_operators() {
        let analysis = analyze("echo key >> ~/.ssh/authorized_keys");
        assert_eq!(analysis.class, SafeDev);
        assert_eq!(analysis.paths.len(), 1);
        assert!(analysis.paths[0].write);
        assert_eq!(classify("cat x > /dev/disk0"), Catastrophic);
        assert_eq!(classify("cargo test &> log.txt"), SafeDev);
    }

    #[test]
    fn fork_bombs() {
        assert_eq!(classify(":(){ :|:& };:"), Catastrophic);
        assert_eq!(classify("bomb() { bomb | bomb & }; bomb"), Catastrophic);
        assert_eq!(classify("function f { f|f& }; f"), Catastrophic);
    }

    #[test]
    fn sudo_is_never_safe() {
        assert_eq!(classify("sudo pnpm test"), Sensitive);
        assert_eq!(classify("sudo -u root systemctl stop nginx"), Sensitive);
        assert_eq!(classify("sudo rm -rf /"), Catastrophic);
    }

    #[test]
    fn deploy_and_network() {
        assert_eq!(classify("git push origin main"), Deploy);
        assert_eq!(classify("ssh host"), NetworkRemote);
    }

    #[test]
    fn label_round_trips() {
        for class in [
            SafeDev,
            Unknown,
            Deploy,
            NetworkRemote,
            Sensitive,
            Catastrophic,
        ] {
            assert_eq!(CommandClass::from_label(class.label()), Some(class));
        }
        assert!(Catastrophic.auto_forbidden());
        assert!(!SafeDev.auto_forbidden());
        assert!(CommandClass::from_label("nope").is_none());
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let mut command = String::from("ls");
        for _ in 0..200 {
            command = format!("$({command})");
        }
        assert!(classify(&command) >= Unknown);
    }
}
