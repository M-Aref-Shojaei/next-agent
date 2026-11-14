// Design adapted from opencode (MIT, © 2025 opencode): packages/core/src/agent.ts, packages/core/src/config/plugin/agent.ts @ cbe71595e1f370ff91b3081872b96bbe56519c41
//! Agent profiles: Markdown files at `.next-agent/agents/<name>.md` plus the
//! built-ins `build`, `plan`, `general` and `explore`.
//!
//! ```markdown
//! ---
//! mode: subagent            # primary | subagent | all (default all)
//! description: Reviews diffs for security problems
//! model_role: review        # or `model: provider/model-id`
//! steps: 20                 # model turns; the last turn is a no-tools summary
//! permissions:
//!   run_command: ask        # allow | ask | deny, per tool name or policy kind
//!   write_file: deny        # `write_file` also covers apply_patch and edit
//!   "*": ask                # anything not listed
//! ---
//! You are a careful security reviewer…   (the body joins the system prompt)
//! ```
//!
//! **Permissions only tighten.** A profile becomes a [`Floor`] the broker
//! applies on top of the permission engine: `deny` blocks the tool (and drops
//! it from the schemas), `ask` forces an approval prompt, and `allow` is the
//! same as not mentioning the tool — the engine still decides. A workspace
//! file that reuses a built-in name is merged with the built-in, keeping the
//! stricter permission for every tool and the smaller step cap, so a
//! repository cannot unlock `plan` or `explore`.
//!
//! Workspace profile files are repository content: treat their prompts as
//! untrusted. They cannot widen what the agent may do.

use crate::runner::Floor;
use crate::tools::ToolRequest;
use std::collections::BTreeMap;
use std::path::Path;

/// Directory (under the workspace root) holding profile files.
pub const PROFILE_DIR: &str = ".next-agent/agents";

/// Where a profile may be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMode {
    /// Selectable for a task, not spawnable as a subagent.
    Primary,
    /// Spawnable through the `subagent` tool only.
    Subagent,
    All,
}

impl ProfileMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Subagent => "subagent",
            Self::All => "all",
        }
    }

    #[must_use]
    pub fn primary(self) -> bool {
        matches!(self, Self::Primary | Self::All)
    }

    #[must_use]
    pub fn subagent(self) -> bool {
        matches!(self, Self::Subagent | Self::All)
    }
}

/// A per-tool permission override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}

impl Permission {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "ask" => Self::Ask,
            "deny" => Self::Deny,
            _ => return None,
        })
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

/// One agent profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub name: String,
    pub mode: ProfileMode,
    pub description: String,
    /// Explicit model id (wins over `model_role`).
    pub model: Option<String>,
    /// Gateway model role (`chat`, `summary`, `review`, …).
    pub model_role: Option<String>,
    /// Tool name or policy kind (`write_file`, `run_command`, …) or `*`.
    pub permissions: BTreeMap<String, Permission>,
    /// Model-turn cap; the last turn is a forced no-tools summary.
    pub steps: Option<usize>,
    /// Extra system prompt text.
    pub prompt: String,
    pub builtin: bool,
}

/// A profile file that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    pub name: String,
    pub message: String,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "agent profile `{}`: {}", self.name, self.message)
    }
}

impl std::error::Error for ProfileError {}

impl AgentProfile {
    fn new(name: &str, mode: ProfileMode, description: &str) -> Self {
        Self {
            name: name.to_string(),
            mode,
            description: description.to_string(),
            model: None,
            model_role: None,
            permissions: BTreeMap::new(),
            steps: None,
            prompt: String::new(),
            builtin: true,
        }
    }

    fn deny(mut self, tools: &[&str]) -> Self {
        for tool in tools {
            self.permissions
                .insert((*tool).to_string(), Permission::Deny);
        }
        self
    }

    fn prompt(mut self, text: &str) -> Self {
        text.clone_into(&mut self.prompt);
        self
    }

    /// The override for a tool: exact agent-facing name, then policy kind,
    /// then `*`, else `Allow` (no override).
    #[must_use]
    pub fn permission(&self, name: &str, kind: &str) -> Permission {
        self.permissions
            .get(name)
            .or_else(|| self.permissions.get(kind))
            .or_else(|| self.permissions.get("*"))
            .copied()
            .unwrap_or(Permission::Allow)
    }

    /// Whether the tool is offered to the model at all.
    #[must_use]
    pub fn offers(&self, name: &str, kind: &str) -> bool {
        self.permission(name, kind) != Permission::Deny
    }

    /// Whether the profile never mutates (no writes, no commands).
    #[must_use]
    pub fn read_only(&self) -> bool {
        !self.offers("write_file", "write_file")
            && !self.offers("apply_patch", "write_file")
            && !self.offers("edit", "write_file")
            && !self.offers("run_command", "run_command")
    }

    /// Merge `other` (a workspace file reusing this name) into this profile,
    /// keeping the stricter permission per tool and the smaller step cap.
    #[must_use]
    pub fn tightened_by(&self, other: &Self) -> Self {
        let mut merged = self.clone();
        let keys: Vec<String> = self
            .permissions
            .keys()
            .chain(other.permissions.keys())
            .cloned()
            .collect();
        for key in keys {
            let (name, kind) = (key.as_str(), policy_kind(&key));
            let stricter = self
                .permission(name, kind)
                .max(other.permission(name, kind));
            merged.permissions.insert(key, stricter);
        }
        merged.steps = match (self.steps, other.steps) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if !other.description.is_empty() {
            merged.description.clone_from(&other.description);
        }
        if !other.prompt.trim().is_empty() {
            merged.prompt = format!("{}\n\n{}", self.prompt, other.prompt)
                .trim()
                .to_string();
        }
        merged.model = other.model.clone().or_else(|| self.model.clone());
        merged.model_role = other.model_role.clone().or_else(|| self.model_role.clone());
        merged
    }
}

/// The strictest override across a chain of profiles (a task's profile and,
/// for subagents, every ancestor's), as a broker [`Floor`].
#[must_use]
pub fn floor_for(chain: &[AgentProfile], request: &ToolRequest) -> Option<Floor> {
    floor_for_tool(chain, request.name(), request.kind())
}

/// [`floor_for`] by tool name and policy kind.
#[must_use]
pub fn floor_for_tool(chain: &[AgentProfile], name: &str, kind: &str) -> Option<Floor> {
    let strictest = chain
        .iter()
        .map(|profile| (profile.permission(name, kind), profile.name.as_str()))
        .max_by_key(|(permission, _)| *permission)?;
    match strictest {
        (Permission::Allow, _) => None,
        (Permission::Ask, profile) => Some(Floor::Ask(format!(
            "agent profile `{profile}` requires approval for {name}"
        ))),
        (Permission::Deny, profile) => Some(Floor::Deny(format!(
            "agent profile `{profile}` does not allow {name}"
        ))),
    }
}

/// Whether every profile in the chain offers the tool.
#[must_use]
pub fn chain_offers(chain: &[AgentProfile], name: &str, kind: &str) -> bool {
    chain.iter().all(|profile| profile.offers(name, kind))
}

const READ_ONLY_TOOLS: [&str; 4] = ["write_file", "apply_patch", "edit", "run_command"];

/// The built-in profiles.
#[must_use]
pub fn builtin_profiles() -> Vec<AgentProfile> {
    vec![
        AgentProfile::new(
            "build",
            ProfileMode::Primary,
            "Default agent: reads, edits and runs commands as the permission mode allows.",
        ),
        AgentProfile::new(
            "plan",
            ProfileMode::Primary,
            "Read-only planning: investigates and proposes a Markdown plan without changing anything.",
        )
        .deny(&READ_ONLY_TOOLS)
        .prompt("You are planning, not implementing. Investigate with read-only tools and finish with a concise Markdown plan."),
        AgentProfile::new(
            "general",
            ProfileMode::Subagent,
            "General-purpose subagent for multi-step research or implementation; cannot spawn subagents.",
        )
        .deny(&["subagent"]),
        AgentProfile::new(
            "explore",
            ProfileMode::Subagent,
            "Fast read-only codebase exploration: finds files, symbols and answers questions about the code.",
        )
        .deny(&READ_ONLY_TOOLS)
        .deny(&["subagent"])
        .prompt("You are exploring a codebase read-only. Search broadly, read only what you need, and report concrete findings with file paths and line numbers."),
    ]
}

/// Parse one profile file. `name` is the file stem.
///
/// # Errors
/// [`ProfileError`] for an invalid name, malformed frontmatter, or an
/// unknown `mode` / permission value.
pub fn parse_profile(name: &str, text: &str) -> Result<AgentProfile, ProfileError> {
    let fail = |message: String| ProfileError {
        name: name.to_string(),
        message,
    };
    if !valid_name(name) {
        return Err(fail(
            "names use lowercase letters, digits, `-` and `_` (at most 64)".into(),
        ));
    }
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n");
    let (front, body) = split_frontmatter(&text);
    let mut profile = AgentProfile::new(name, ProfileMode::All, "");
    profile.builtin = false;
    profile.prompt = body.trim().to_string();
    let mut in_permissions = false;
    for (number, raw) in front.lines().enumerate() {
        let line = strip_comment(raw);
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let Some((key, value)) = line.trim().split_once(':') else {
            return Err(fail(format!("line {}: expected `key: value`", number + 1)));
        };
        let key = unquote(key.trim());
        let value = unquote(value.trim());
        if indented && in_permissions {
            let permission = Permission::parse(&value).ok_or_else(|| {
                fail(format!(
                    "line {}: permission for `{key}` must be allow, ask or deny",
                    number + 1
                ))
            })?;
            profile.permissions.insert(key, permission);
            continue;
        }
        in_permissions = false;
        match key.as_str() {
            "mode" => {
                profile.mode = match value.as_str() {
                    "primary" => ProfileMode::Primary,
                    "subagent" => ProfileMode::Subagent,
                    "all" | "" => ProfileMode::All,
                    other => return Err(fail(format!("unknown mode `{other}`"))),
                }
            }
            "description" => profile.description = value,
            "model" => profile.model = (!value.is_empty()).then_some(value),
            "model_role" | "role" => profile.model_role = (!value.is_empty()).then_some(value),
            "steps" => {
                let steps: usize = value
                    .parse()
                    .map_err(|_| fail(format!("steps must be a positive number, not `{value}`")))?;
                profile.steps = Some(steps.max(1));
            }
            "permissions" | "permission" => {
                if value.starts_with('{') {
                    for (tool, permission) in parse_flow_map(&value).map_err(fail)? {
                        profile.permissions.insert(tool, permission);
                    }
                } else if value.is_empty() {
                    in_permissions = true;
                } else {
                    // `permissions: deny` applies to every tool.
                    let permission = Permission::parse(&value)
                        .ok_or_else(|| fail(format!("unknown permission `{value}`")))?;
                    profile.permissions.insert("*".into(), permission);
                }
            }
            // Unknown keys are ignored so newer files still load.
            _ => {}
        }
    }
    Ok(profile)
}

/// Built-ins merged with the workspace's `.next-agent/agents/*.md` files.
/// Unreadable or invalid files are reported, never fatal.
#[must_use]
pub fn load_profiles(workspace_root: &Path) -> (Vec<AgentProfile>, Vec<ProfileError>) {
    let mut profiles = builtin_profiles();
    let mut errors = Vec::new();
    let dir = workspace_root.join(PROFILE_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (profiles, errors);
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    for path in files {
        let name = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let parsed = std::fs::metadata(&path)
            .map_err(|error| error.to_string())
            .and_then(|meta| {
                if meta.len() > 256 * 1024 {
                    Err("file is larger than 256 KiB".into())
                } else {
                    std::fs::read_to_string(&path).map_err(|error| error.to_string())
                }
            })
            .map_err(|message| ProfileError {
                name: name.clone(),
                message,
            })
            .and_then(|text| parse_profile(&name, &text));
        match parsed {
            Ok(profile) => {
                if let Some(existing) = profiles.iter_mut().find(|p| p.name == profile.name) {
                    *existing = existing.tightened_by(&profile);
                } else {
                    profiles.push(profile);
                }
            }
            Err(error) => errors.push(error),
        }
    }
    (profiles, errors)
}

/// Look up a profile by name.
#[must_use]
pub fn find<'a>(profiles: &'a [AgentProfile], name: &str) -> Option<&'a AgentProfile> {
    profiles.iter().find(|profile| profile.name == name)
}

/// The policy kind a tool name falls under (`edit` and `apply_patch` are
/// writes); other names are their own kind.
#[must_use]
pub fn policy_kind(name: &str) -> &str {
    match name {
        "apply_patch" | "edit" => "write_file",
        other => other,
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return ("", text);
    };
    if let Some(end) = rest.find("\n---\n") {
        return (&rest[..end], &rest[end + 5..]);
    }
    if let Some(front) = rest.strip_suffix("\n---") {
        return (front, "");
    }
    ("", text)
}

fn strip_comment(line: &str) -> &str {
    // A `#` starts a comment unless it sits inside quotes.
    let mut quote: Option<char> = None;
    for (index, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (None, '#') if index == 0 || line[..index].ends_with(char::is_whitespace) => {
                return &line[..index]
            }
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_string();
        }
    }
    value.to_string()
}

fn parse_flow_map(value: &str) -> Result<Vec<(String, Permission)>, String> {
    let inner = value
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| format!("unterminated map `{value}`"))?;
    let mut out = Vec::new();
    for pair in inner.split(',').filter(|pair| !pair.trim().is_empty()) {
        let (key, value) = pair
            .split_once(':')
            .ok_or_else(|| format!("expected `tool: permission` in `{pair}`"))?;
        let permission = Permission::parse(&unquote(value))
            .ok_or_else(|| format!("unknown permission in `{pair}`"))?;
        out.push((unquote(key), permission));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(path: &str) -> ToolRequest {
        ToolRequest::Write {
            path: PathBuf::from(path),
            contents: String::new(),
        }
    }

    #[test]
    fn builtins_have_the_expected_shape() {
        let profiles = builtin_profiles();
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["build", "plan", "general", "explore"]);
        let plan = find(&profiles, "plan").expect("plan");
        assert!(plan.read_only());
        assert!(plan.mode.primary() && !plan.mode.subagent());
        assert!(plan.offers("subagent", "subagent"));
        let explore = find(&profiles, "explore").expect("explore");
        assert!(explore.read_only());
        assert!(!explore.offers("subagent", "subagent"));
        let general = find(&profiles, "general").expect("general");
        assert!(!general.read_only());
        assert!(!general.offers("subagent", "subagent"));
        assert!(!find(&profiles, "build").expect("build").read_only());
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let text = "\u{feff}---\r\nmode: subagent\r\ndescription: \"Reviews code\" # trailing comment\r\nmodel_role: review\r\nsteps: 7\r\npermissions:\r\n  run_command: ask\r\n  write_file: deny\r\n  \"*\": allow\r\n---\r\nYou review.\r\n";
        let profile = parse_profile("reviewer", text).expect("parse");
        assert_eq!(profile.mode, ProfileMode::Subagent);
        assert_eq!(profile.description, "Reviews code");
        assert_eq!(profile.model_role.as_deref(), Some("review"));
        assert_eq!(profile.steps, Some(7));
        assert_eq!(profile.prompt, "You review.");
        assert_eq!(
            profile.permission("run_command", "run_command"),
            Permission::Ask
        );
        // `write_file` covers edit and apply_patch through the policy kind.
        assert_eq!(profile.permission("edit", "write_file"), Permission::Deny);
        assert_eq!(
            profile.permission("read_file", "read_file"),
            Permission::Allow
        );
        assert!(!profile.builtin);
    }

    #[test]
    fn flow_maps_and_blanket_permissions() {
        let flow = parse_profile(
            "a",
            "---\npermissions: {edit: deny, run_command: ask}\n---\n",
        )
        .expect("flow");
        assert_eq!(flow.permission("edit", "write_file"), Permission::Deny);
        assert_eq!(
            flow.permission("write_file", "write_file"),
            Permission::Allow
        );
        let blanket = parse_profile("b", "---\npermissions: ask\n---\nx").expect("blanket");
        assert_eq!(
            blanket.permission("read_file", "read_file"),
            Permission::Ask
        );
        let none = parse_profile("c", "just a prompt").expect("no frontmatter");
        assert_eq!(none.prompt, "just a prompt");
        assert_eq!(none.mode, ProfileMode::All);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse_profile("Bad Name", "").is_err());
        assert!(parse_profile("a", "---\nmode: boss\n---\n").is_err());
        assert!(parse_profile("a", "---\npermissions:\n  edit: maybe\n---\n").is_err());
        assert!(parse_profile("a", "---\nsteps: many\n---\n").is_err());
        assert!(parse_profile("a", "---\nnot a pair\n---\n").is_err());
    }

    #[test]
    fn workspace_files_cannot_loosen_builtins() {
        let plan = find(&builtin_profiles(), "plan").expect("plan").clone();
        let sneaky = parse_profile(
            "plan",
            "---\nsteps: 3\npermissions:\n  write_file: allow\n  run_command: allow\n  http_fetch: deny\n---\nextra",
        )
        .expect("parse");
        let merged = plan.tightened_by(&sneaky);
        let explicit = parse_profile("x", "---\npermissions:\n  edit: allow\n---\n").expect("x");
        let narrowed = explicit.tightened_by(
            &parse_profile("x", "---\npermissions:\n  write_file: deny\n---\n").expect("y"),
        );
        assert_eq!(narrowed.permission("edit", "write_file"), Permission::Deny);
        assert!(merged.read_only(), "{:?}", merged.permissions);
        assert_eq!(
            merged.permission("http_fetch", "http_fetch"),
            Permission::Deny
        );
        assert_eq!(merged.steps, Some(3));
        assert!(merged.prompt.ends_with("extra"));
    }

    #[test]
    fn floors_take_the_strictest_profile_in_the_chain() {
        let profiles = builtin_profiles();
        let build = find(&profiles, "build").expect("build").clone();
        let explore = find(&profiles, "explore").expect("explore").clone();
        let asking =
            parse_profile("asker", "---\npermissions:\n  read_file: ask\n---\n").expect("p");
        assert_eq!(
            floor_for(std::slice::from_ref(&build), &write("/w/a")),
            None
        );
        assert!(matches!(
            floor_for(&[build.clone(), explore.clone()], &write("/w/a")),
            Some(Floor::Deny(reason)) if reason.contains("explore")
        ));
        let read = ToolRequest::Read {
            path: PathBuf::from("/w/a"),
        };
        assert!(matches!(
            floor_for(&[asking.clone(), build.clone()], &read),
            Some(Floor::Ask(_))
        ));
        // Deny anywhere in the chain beats ask anywhere else.
        assert!(matches!(
            floor_for(&[asking, explore], &write("/w/a")),
            Some(Floor::Deny(_))
        ));
        assert!(!chain_offers(
            &[
                build.clone(),
                find(&profiles, "general").expect("g").clone()
            ],
            "subagent",
            "subagent"
        ));
        assert!(chain_offers(&[build], "subagent", "subagent"));
    }

    #[test]
    fn loads_workspace_directory() {
        let root = std::env::temp_dir().join(format!("na-profiles-{}", std::process::id()));
        let dir = root.join(PROFILE_DIR);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("reviewer.md"), "---\nmode: subagent\n---\nReview.").expect("w");
        std::fs::write(dir.join("BAD.md"), "x").expect("w");
        std::fs::write(dir.join("notes.txt"), "ignored").expect("w");
        std::fs::write(
            dir.join("explore.md"),
            "---\npermissions:\n  write_file: allow\n---\n",
        )
        .expect("w");
        let (profiles, errors) = load_profiles(&root);
        assert!(find(&profiles, "reviewer").is_some());
        assert!(find(&profiles, "explore").expect("explore").read_only());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].name, "BAD");
        let (none, no_errors) = load_profiles(&root.join("missing"));
        assert_eq!(none.len(), 4);
        assert_eq!(no_errors, Vec::<ProfileError>::new());
        let _ = std::fs::remove_dir_all(&root);
    }
}
