//! User rules: the `allow` / `deny` entries a person adds explicitly.
//!
//! Matching is deliberately asymmetric:
//! - a **deny** rule matches if it matches *any* part of the request (any
//!   simple command of a compound line, any path a command mentions);
//! - an **allow** rule matches only if it covers *every* part (every simple
//!   command; the file tool's own path), so `pnpm test && curl … | sh` is not
//!   unlocked by an allow rule for `pnpm test`.

/// Whether a matching rule permits or forbids the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

/// What a rule matches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleTarget {
    /// Any request using this tool, e.g. `run_command`.
    Tool { kind: String },
    /// Paths matching a glob, e.g. `~/scratch/**`. Relative globs are anchored
    /// at the workspace root. `*` stays within a segment, `**` crosses
    /// segments, `?` is one character, `[abc]`/`[!abc]` is a class (use `[*]`
    /// for a literal `*`).
    Path { glob: String },
    /// Commands whose leading words match, e.g. `["pnpm", "test"]`. The first
    /// word is compared as a lower-case program basename.
    CommandPrefix { prefix: Vec<String> },
}

/// Where a rule applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    /// Only inside one workspace, addressed by its stable `ws_…` id.
    Workspace {
        id: String,
    },
}

/// One persisted rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub effect: Effect,
    pub target: RuleTarget,
    pub scope: Scope,
}

impl Rule {
    #[must_use]
    pub fn allow(id: impl Into<String>, target: RuleTarget) -> Self {
        Self {
            id: id.into(),
            effect: Effect::Allow,
            target,
            scope: Scope::Global,
        }
    }

    #[must_use]
    pub fn deny(id: impl Into<String>, target: RuleTarget) -> Self {
        Self {
            id: id.into(),
            effect: Effect::Deny,
            target,
            scope: Scope::Global,
        }
    }

    #[must_use]
    pub fn scoped_to(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Whether this rule applies to the given workspace (global rules always do).
    #[must_use]
    pub fn applies_to(&self, workspace_id: &str) -> bool {
        match &self.scope {
            Scope::Global => true,
            Scope::Workspace { id } => id == workspace_id,
        }
    }
}

/// All rules in effect for a session.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    #[must_use]
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Keep only the rules `keep` accepts.
    pub fn retain(&mut self, keep: impl FnMut(&Rule) -> bool) {
        self.rules.retain(keep);
    }

    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter()
    }

    /// First rule with the given effect that matches `subject` in `workspace`.
    #[must_use]
    pub fn matching(
        &self,
        effect: Effect,
        subject: &Subject<'_>,
        workspace_id: &str,
    ) -> Option<&Rule> {
        self.rules.iter().find(|rule| {
            rule.effect == effect
                && rule.applies_to(workspace_id)
                && match effect {
                    Effect::Deny => matches_any(rule, subject),
                    Effect::Allow => matches_all(rule, subject),
                }
        })
    }
}

/// How path globs are interpreted.
#[derive(Debug, Clone, Default)]
pub struct GlobEnv {
    /// Replaces a leading `~` in patterns.
    pub home: Option<String>,
    /// Anchor for relative patterns.
    pub root: Option<String>,
    /// Treat `\` as a separator too (Windows).
    pub backslash_separator: bool,
    pub case_insensitive: bool,
}

/// The parts of a request that rules can match against.
#[derive(Debug, Clone)]
pub struct Subject<'a> {
    pub tool: &'a str,
    /// The file tool's own target (canonical and, if different, lexical).
    pub own_paths: &'a [String],
    /// Paths a command mentions (and its cwd). Deny rules only.
    pub mentioned_paths: &'a [String],
    /// Effective argv of each simple command.
    pub commands: &'a [Vec<String>],
    /// The command line could not be fully parsed: allow rules never match.
    pub opaque: bool,
    pub globs: &'a GlobEnv,
}

fn matches_any(rule: &Rule, subject: &Subject<'_>) -> bool {
    match &rule.target {
        RuleTarget::Tool { kind } => kind == subject.tool,
        RuleTarget::Path { glob } => subject
            .own_paths
            .iter()
            .chain(subject.mentioned_paths)
            .any(|path| path_matches(glob, path, subject.globs)),
        RuleTarget::CommandPrefix { prefix } => subject
            .commands
            .iter()
            .any(|argv| prefix_matches(prefix, argv)),
    }
}

fn matches_all(rule: &Rule, subject: &Subject<'_>) -> bool {
    match &rule.target {
        RuleTarget::Tool { kind } => kind == subject.tool,
        RuleTarget::Path { glob } => {
            !subject.own_paths.is_empty()
                && subject
                    .own_paths
                    .iter()
                    .all(|path| path_matches(glob, path, subject.globs))
        }
        RuleTarget::CommandPrefix { prefix } => {
            !subject.opaque
                && !subject.commands.is_empty()
                && subject
                    .commands
                    .iter()
                    .all(|argv| prefix_matches(prefix, argv))
        }
    }
}

fn prefix_matches(prefix: &[String], argv: &[String]) -> bool {
    if prefix.is_empty() || prefix.len() > argv.len() {
        return false;
    }
    prefix
        .iter()
        .zip(argv)
        .enumerate()
        .all(|(index, (want, got))| {
            if index == 0 {
                program_basename(want) == *got
            } else {
                want == got
            }
        })
}

fn program_basename(word: &str) -> String {
    word.rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .to_lowercase()
}

fn path_matches(glob: &str, path: &str, env: &GlobEnv) -> bool {
    let sep = if env.backslash_separator { '\\' } else { '/' };
    let mut pattern = glob.to_string();
    if let (Some(home), Some(rest)) = (&env.home, pattern.strip_prefix('~')) {
        if rest.is_empty() || rest.starts_with(['/', '\\']) {
            pattern = format!("{home}{rest}");
        }
    }
    let absolute = pattern.starts_with(['/', '\\'])
        || pattern.as_bytes().get(1) == Some(&b':')
        || pattern.starts_with("**");
    if !absolute {
        if let Some(root) = &env.root {
            pattern = format!("{root}{sep}{pattern}");
        }
    }
    let (pattern, path) = if env.backslash_separator {
        (pattern.replace('/', "\\"), path.replace('/', "\\"))
    } else {
        (pattern, path.to_string())
    };
    let (pattern, path) = if env.case_insensitive {
        (pattern.to_lowercase(), path.to_lowercase())
    } else {
        (pattern, path)
    };
    glob_match_with(&pattern, &path, sep)
}

/// Anchored glob with `/` as separator: `*` matches within one path segment,
/// `**` matches across segments, `?` one character, `[…]` a class.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_with(pattern, text, '/')
}

fn glob_match_with(pattern: &str, text: &str, sep: char) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut memo = vec![None; (pattern.len() + 1) * (text.len() + 1)];
    glob_from(&pattern, 0, &text, 0, sep, &mut memo)
}

/// Escape a literal path so it can be stored as a [`RuleTarget::Path`] glob.
#[must_use]
pub fn escape_glob(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    for ch in literal.chars() {
        match ch {
            '*' | '?' | '[' | ']' => {
                out.push('[');
                out.push(ch);
                out.push(']');
            }
            other => out.push(other),
        }
    }
    out
}

fn glob_from(
    pattern: &[char],
    p: usize,
    text: &[char],
    t: usize,
    sep: char,
    memo: &mut [Option<bool>],
) -> bool {
    let key = p * (text.len() + 1) + t;
    if let Some(known) = memo[key] {
        return known;
    }
    let result = glob_step(pattern, p, text, t, sep, memo);
    memo[key] = Some(result);
    result
}

fn glob_step(
    pattern: &[char],
    p: usize,
    text: &[char],
    t: usize,
    sep: char,
    memo: &mut [Option<bool>],
) -> bool {
    if p == pattern.len() {
        return t == text.len();
    }
    match pattern[p] {
        '*' if pattern.get(p + 1) == Some(&'*') => {
            let rest = p + 2;
            if pattern.get(rest) == Some(&sep) {
                // `**/` matches zero or more whole segments.
                let tail = rest + 1;
                if glob_from(pattern, tail, text, t, sep, memo) {
                    return true;
                }
                return (t..text.len())
                    .filter(|&index| text[index] == sep)
                    .any(|index| glob_from(pattern, tail, text, index + 1, sep, memo));
            }
            (t..=text.len()).any(|skip| glob_from(pattern, rest, text, skip, sep, memo))
        }
        '*' => {
            let limit = text[t..]
                .iter()
                .position(|c| *c == sep)
                .map_or(text.len(), |offset| t + offset);
            (t..=limit).any(|skip| glob_from(pattern, p + 1, text, skip, sep, memo))
        }
        '?' => {
            t < text.len() && text[t] != sep && glob_from(pattern, p + 1, text, t + 1, sep, memo)
        }
        '[' => match class_end(pattern, p) {
            Some(end) => {
                t < text.len()
                    && text[t] != sep
                    && class_matches(&pattern[p + 1..end], text[t])
                    && glob_from(pattern, end + 1, text, t + 1, sep, memo)
            }
            None => {
                t < text.len()
                    && text[t] == '['
                    && glob_from(pattern, p + 1, text, t + 1, sep, memo)
            }
        },
        literal => {
            t < text.len()
                && text[t] == literal
                && glob_from(pattern, p + 1, text, t + 1, sep, memo)
        }
    }
}

/// Index of the `]` closing a class that opens at `start`.
fn class_end(pattern: &[char], start: usize) -> Option<usize> {
    let mut index = start + 1;
    if matches!(pattern.get(index), Some('!' | '^')) {
        index += 1;
    }
    // A `]` right after the opening (or negation) is a literal member.
    if pattern.get(index) == Some(&']') {
        index += 1;
    }
    while index < pattern.len() {
        if pattern[index] == ']' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn class_matches(body: &[char], ch: char) -> bool {
    let (negated, body) = match body.first() {
        Some('!' | '^') => (true, &body[1..]),
        _ => (false, body),
    };
    let mut found = false;
    let mut index = 0;
    while index < body.len() {
        if index + 2 < body.len() && body[index + 1] == '-' {
            if body[index] <= ch && ch <= body[index + 2] {
                found = true;
            }
            index += 3;
        } else {
            if body[index] == ch {
                found = true;
            }
            index += 1;
        }
    }
    found != negated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> GlobEnv {
        GlobEnv {
            home: Some("/home/dev".into()),
            root: Some("/w".into()),
            backslash_separator: false,
            case_insensitive: false,
        }
    }

    fn command_subject<'a>(commands: &'a [Vec<String>], globs: &'a GlobEnv) -> Subject<'a> {
        Subject {
            tool: "run_command",
            own_paths: &[],
            mentioned_paths: &[],
            commands,
            opaque: false,
            globs,
        }
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn tool_rules_match_exactly() {
        let set = RuleSet::new(vec![Rule::deny(
            "r1",
            RuleTarget::Tool {
                kind: "run_command".into(),
            },
        )]);
        let globs = env();
        let commands = vec![argv(&["ls"])];
        let hit = command_subject(&commands, &globs);
        let miss = Subject {
            tool: "read_file",
            ..hit.clone()
        };
        assert!(set.matching(Effect::Deny, &hit, "ws_1").is_some());
        assert!(set.matching(Effect::Deny, &miss, "ws_1").is_none());
    }

    #[test]
    fn command_prefix_matches_only_that_subcommand() {
        let rule = Rule::allow(
            "r",
            RuleTarget::CommandPrefix {
                prefix: vec!["pnpm".into(), "test".into()],
            },
        );
        let set = RuleSet::new(vec![rule]);
        let globs = env();
        let test = vec![argv(&["pnpm", "test", "--watch"])];
        let install = vec![argv(&["pnpm", "install"])];
        let compound = vec![argv(&["pnpm", "test"]), argv(&["curl", "x"])];
        assert!(set
            .matching(Effect::Allow, &command_subject(&test, &globs), "ws_1")
            .is_some());
        assert!(set
            .matching(Effect::Allow, &command_subject(&install, &globs), "ws_1")
            .is_none());
        assert!(
            set.matching(Effect::Allow, &command_subject(&compound, &globs), "ws_1")
                .is_none(),
            "an allow rule must cover every simple command"
        );
        let opaque = Subject {
            opaque: true,
            ..command_subject(&test, &globs)
        };
        assert!(set.matching(Effect::Allow, &opaque, "ws_1").is_none());
    }

    #[test]
    fn deny_prefix_matches_any_segment_and_program_paths() {
        let set = RuleSet::new(vec![Rule::deny(
            "no-push",
            RuleTarget::CommandPrefix {
                prefix: vec!["git".into(), "push".into()],
            },
        )]);
        let globs = env();
        let compound = vec![argv(&["ls"]), argv(&["git", "push"])];
        assert!(set
            .matching(Effect::Deny, &command_subject(&compound, &globs), "ws_1")
            .is_some());
    }

    #[test]
    fn workspace_rules_do_not_leak() {
        let rule = Rule::allow(
            "r",
            RuleTarget::Tool {
                kind: "write_file".into(),
            },
        )
        .scoped_to(Scope::Workspace { id: "ws_1".into() });
        let set = RuleSet::new(vec![rule]);
        let globs = env();
        let paths = vec!["/w/a".to_string()];
        let subject = Subject {
            tool: "write_file",
            own_paths: &paths,
            mentioned_paths: &[],
            commands: &[],
            opaque: false,
            globs: &globs,
        };
        assert!(set.matching(Effect::Allow, &subject, "ws_1").is_some());
        assert!(set.matching(Effect::Allow, &subject, "ws_2").is_none());
    }

    /// The original assertion `glob_match("*", "/w/a")` contradicted the
    /// documented semantics (and this test's own name): `*` never crosses a
    /// separator, so a bare `*` cannot match a multi-segment path. `**` is the
    /// spelled-out way to match everything. Allowing `*` to cross `/` would
    /// silently widen user rules like `/w/src/*` to the whole subtree.
    #[test]
    fn globs_are_anchored_and_segment_aware() {
        assert!(glob_match("/w/src/*.rs", "/w/src/main.rs"));
        assert!(!glob_match("/w/src/*.rs", "/w/src/nested/main.rs"));
        assert!(glob_match("/w/src/**/*.rs", "/w/src/a/b/main.rs"));
        assert!(!glob_match("/w/src/*.rs", "/w/src/main.ts"));
        assert!(!glob_match("*", "/w/a"));
        assert!(glob_match("**", "/w/a"));
        assert!(glob_match("*", "file"));
        assert!(!glob_match("/w/a", "/w/ab"));
        assert!(glob_match("**/*.rs", "main.rs"));
        assert!(glob_match("**/*.rs", "a/b/main.rs"));
        assert!(!glob_match("/w/?", "/w/ab"));
        assert!(!glob_match("/w?a", "/w/a"));
    }

    #[test]
    fn glob_classes_and_escapes() {
        assert!(glob_match("/w/[abc].rs", "/w/b.rs"));
        assert!(!glob_match("/w/[!abc].rs", "/w/b.rs"));
        assert!(glob_match("/w/[a-c].rs", "/w/c.rs"));
        assert!(glob_match(&escape_glob("/w/a*b?.rs"), "/w/a*b?.rs"));
        assert!(!glob_match(&escape_glob("/w/a*b"), "/w/axxb"));
        assert!(glob_match("/w/[", "/w/["));
    }

    #[test]
    fn glob_is_not_exponential() {
        let pattern = "/**/**/**/**/**/**/**/**/**/**/x";
        let text = format!("/{}", "a/".repeat(40));
        assert!(!glob_match(pattern, &text));
    }

    #[test]
    fn path_globs_expand_home_and_relative() {
        let globs = env();
        assert!(path_matches(
            "~/scratch/**",
            "/home/dev/scratch/a/b",
            &globs
        ));
        assert!(path_matches("src/**", "/w/src/lib.rs", &globs));
        assert!(!path_matches("src/**", "/other/src/lib.rs", &globs));
        let ci = GlobEnv {
            case_insensitive: true,
            ..env()
        };
        assert!(path_matches("~/.ssh/**", "/home/dev/.SSH/id_rsa", &ci));
        let windows = GlobEnv {
            home: Some("C:\\Users\\dev".into()),
            root: Some("C:\\w".into()),
            backslash_separator: true,
            case_insensitive: true,
        };
        assert!(path_matches(
            "~/.ssh/**",
            "C:\\Users\\dev\\.ssh\\x",
            &windows
        ));
        assert!(path_matches("src/*.rs", "c:\\w\\SRC\\a.rs", &windows));
    }

    #[test]
    fn rule_set_bookkeeping() {
        let mut set = RuleSet::empty();
        assert!(set.is_empty());
        set.push(Rule::deny(
            "d",
            RuleTarget::Tool {
                kind: "run_command".into(),
            },
        ));
        assert_eq!(set.len(), 1);
        assert_eq!(set.iter().count(), 1);
        assert!(!set.is_empty());
    }
}
