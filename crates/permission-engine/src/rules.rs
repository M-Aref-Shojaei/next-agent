//! User rules: the `allow` / `deny` entries a person adds explicitly.

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
    /// Paths matching a glob, e.g. `~/scratch/**`.
    Path { glob: String },
    /// Commands whose leading words match, e.g. `["pnpm", "test"]`.
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
            rule.effect == effect && rule.applies_to(workspace_id) && matches(rule, subject)
        })
    }
}

/// The parts of a request that rules can match against.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    pub tool: &'a str,
    pub path: Option<&'a str>,
    pub command: Option<&'a str>,
}

fn matches(rule: &Rule, subject: &Subject<'_>) -> bool {
    match &rule.target {
        RuleTarget::Tool { kind } => kind == subject.tool,
        RuleTarget::Path { glob } => subject.path.is_some_and(|path| glob_match(glob, path)),
        RuleTarget::CommandPrefix { prefix } => subject
            .command
            .is_some_and(|command| command_matches(prefix, command)),
    }
}

/// Prefix match on the command's leading words, after dropping a leading
/// wrapper (`sudo`, `env`). `["pnpm","test"]` matches `pnpm test --watch`
/// but not `pnpm install`.
fn command_matches(prefix: &[String], command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    let words = skip_wrappers(&words);
    prefix.len() <= words.len() && prefix.iter().zip(words.iter()).all(|(a, b)| a == *b)
}

fn skip_wrappers<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let mut index = 0;
    while index < words.len() && matches!(words[index], "sudo" | "doas" | "env" | "time" | "nohup")
    {
        index += 1;
        while index < words.len() && words[index].contains('=') {
            index += 1;
        }
    }
    &words[index..]
}

/// Minimal glob: `*` matches within one path segment, `**` matches across
/// segments. Anchored at both ends.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_from(&pattern, &text)
}

fn glob_from(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if pattern[0] == '*' {
        if pattern.get(1) == Some(&'*') {
            let rest = &pattern[2..];
            // `**/` matches zero or more whole segments, so `**/*.rs` also
            // matches `main.rs` at the current level.
            if matches!(rest.first(), Some('/' | '\\')) {
                let separator = rest[0];
                let tail = &rest[1..];
                if glob_from(tail, text) {
                    return true;
                }
                return (0..text.len())
                    .filter(|&index| text[index] == separator)
                    .any(|index| glob_from(tail, &text[index + 1..]));
            }
            return (0..=text.len()).any(|skip| glob_from(rest, &text[skip..]));
        }
        let rest = &pattern[1..];
        // A single `*` never crosses a separator; `**` is the escape hatch.
        let limit = text
            .iter()
            .position(|c| *c == '/' || *c == '\\')
            .unwrap_or(text.len());
        return (0..=limit).any(|skip| glob_from(rest, &text[skip..]));
    }
    if text.is_empty() {
        return false;
    }
    if pattern[0] == '?' || pattern[0] == text[0] {
        return glob_from(&pattern[1..], &text[1..]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_rules_match_exactly() {
        let set = RuleSet::new(vec![Rule::deny(
            "r1",
            RuleTarget::Tool {
                kind: "run_command".into(),
            },
        )]);
        let hit = Subject {
            tool: "run_command",
            path: None,
            command: Some("ls"),
        };
        let miss = Subject {
            tool: "read_file",
            path: Some("/w/a"),
            command: None,
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
        let test = Subject {
            tool: "run_command",
            path: None,
            command: Some("pnpm test --watch"),
        };
        let install = Subject {
            tool: "run_command",
            path: None,
            command: Some("pnpm install"),
        };
        let sudo = Subject {
            tool: "run_command",
            path: None,
            command: Some("sudo env CI=1 pnpm test"),
        };
        assert!(set.matching(Effect::Allow, &test, "ws_1").is_some());
        assert!(set.matching(Effect::Allow, &install, "ws_1").is_none());
        assert!(set.matching(Effect::Allow, &sudo, "ws_1").is_some());
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
        let subject = Subject {
            tool: "write_file",
            path: Some("/w/a"),
            command: None,
        };
        assert!(set.matching(Effect::Allow, &subject, "ws_1").is_some());
        assert!(set.matching(Effect::Allow, &subject, "ws_2").is_none());
    }

    #[test]
    fn globs_are_anchored_and_segment_aware() {
        assert!(glob_match("/w/src/*.rs", "/w/src/main.rs"));
        assert!(!glob_match("/w/src/*.rs", "/w/src/nested/main.rs"));
        assert!(glob_match("/w/src/**/*.rs", "/w/src/a/b/main.rs"));
        assert!(!glob_match("/w/src/*.rs", "/w/src/main.ts"));
        assert!(glob_match("*", "/w/a"));
        assert!(!glob_match("/w/a", "/w/ab"));
        assert!(glob_match("**/*.rs", "main.rs"));
        assert!(glob_match("**/*.rs", "a/b/main.rs"));
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
