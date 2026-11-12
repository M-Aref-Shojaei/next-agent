//! Path boundary checks: expand `~`, join relative paths to `cwd`, resolve
//! `..` and symlinks/junctions, and classify the target as inside the
//! workspace, outside it, or on the sensitive list.
//!
//! The crate stays I/O-free. Filesystem facts (what a path canonicalizes to,
//! whether it is a dangling link) come in through [`FsFacts`]: the broker
//! implements it with `std::fs::canonicalize`/`read_link` (via `dunce` on
//! Windows), tests and the UI preview use [`MapFs`], and [`Lexical`] answers
//! "nothing exists", which degrades to pure lexical normalization.
//!
//! Paths are parsed as strings according to a [`PathStyle`] rather than with
//! `std::path`, so the Windows rules (drive letters, UNC, `\\?\`, trailing
//! dots, alternate data streams, device names) are testable on every host.
//!
//! Anything that cannot be resolved with confidence is classified
//! [`Zone::Sensitive`]: unknown means "most restrictive", never "inside".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where a resolved path sits relative to the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Zone {
    /// Inside the workspace root (or active worktree).
    Inside,
    /// Outside the workspace and not on the sensitive list.
    Outside,
    /// On the sensitive list (keys, credentials, system dirs), or a path the
    /// engine could not resolve with confidence.
    Sensitive,
}

impl Zone {
    /// Name used in fixtures and events.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Inside => "inside",
            Self::Outside => "outside",
            Self::Sensitive => "sensitive",
        }
    }
}

/// Path syntax to parse with. Independent of the host so both syntaxes are
/// covered by the same test corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// `/`-separated, single root.
    Posix,
    /// `\` or `/` separated; drive letters, UNC and verbatim prefixes.
    Windows,
}

impl PathStyle {
    /// Style of the machine this code runs on.
    #[must_use]
    pub fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    fn is_sep(self, ch: char) -> bool {
        ch == '/' || (self == Self::Windows && ch == '\\')
    }
}

/// Filesystem facts injected by the caller. Implementations must not be
/// influenced by the request beyond the path they are asked about.
pub trait FsFacts {
    /// Fully resolved form of `path` if it exists (follows every symlink and
    /// junction, like `realpath`). `None` if any component does not exist.
    fn canonicalize(&self, path: &Path) -> Option<PathBuf>;

    /// Target of `path` if `path` itself is a symlink or junction, even a
    /// dangling one. Used to refuse writes through dangling links.
    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let _ = path;
        None
    }
}

/// Knows nothing about the disk: every path is treated as not existing, so
/// resolution is purely lexical.
#[derive(Debug, Clone, Copy, Default)]
pub struct Lexical;

impl FsFacts for Lexical {
    fn canonicalize(&self, _path: &Path) -> Option<PathBuf> {
        None
    }
}

/// In-memory filesystem facts for tests and UI previews.
///
/// Every listed entry and all of its ancestors exist. Symlink targets may be
/// absolute or relative to the link's parent.
#[derive(Debug, Clone)]
pub struct MapFs {
    style: PathStyle,
    entries: BTreeSet<String>,
    links: BTreeMap<String, String>,
}

impl MapFs {
    #[must_use]
    pub fn new(style: PathStyle) -> Self {
        Self {
            style,
            entries: BTreeSet::new(),
            links: BTreeMap::new(),
        }
    }

    /// Register an existing file or directory (and implicitly its ancestors).
    #[must_use]
    pub fn with_entry(mut self, path: &str) -> Self {
        if let Ok(Parsed::Absolute(abs, _)) = parse(path, self.style) {
            let mut acc = AbsPath {
                prefix: abs.prefix.clone(),
                parts: Vec::new(),
            };
            for part in abs.parts {
                acc.parts.push(part);
                self.entries.insert(acc.render(self.style));
            }
        }
        self
    }

    /// Register `path` as a symlink (or junction) pointing at `target`.
    #[must_use]
    pub fn with_link(mut self, path: &str, target: &str) -> Self {
        if let Ok(Parsed::Absolute(abs, _)) = parse(path, self.style) {
            let key = abs.render(self.style);
            if let Some(parent) = abs.parent() {
                let rendered = parent.render(self.style);
                self = self.with_entry(&rendered);
            }
            self.links.insert(key, target.to_string());
        }
        self
    }

    fn resolve_inner(&self, path: &AbsPath, budget: &mut u32) -> Option<AbsPath> {
        let mut acc = AbsPath {
            prefix: path.prefix.clone(),
            parts: Vec::new(),
        };
        for part in &path.parts {
            if part == ".." {
                acc.parts.pop();
                continue;
            }
            acc.parts.push(part.clone());
            let key = acc.render(self.style);
            if let Some(target) = self.links.get(&key) {
                *budget = budget.checked_sub(1)?;
                let base = acc.parent().unwrap_or_else(|| acc.clone());
                let joined = join_raw(&base, target, self.style)?;
                acc = self.resolve_inner(&joined, budget)?;
            } else if !self.entries.contains(&key) {
                return None;
            }
        }
        Some(acc)
    }
}

impl FsFacts for MapFs {
    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let Ok(Parsed::Absolute(abs, _)) = parse(&path.to_string_lossy(), self.style) else {
            return None;
        };
        let mut budget = 40;
        self.resolve_inner(&abs, &mut budget)
            .map(|resolved| PathBuf::from(resolved.render(self.style)))
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        let Ok(Parsed::Absolute(abs, _)) = parse(&path.to_string_lossy(), self.style) else {
            return None;
        };
        self.links
            .get(&abs.render(self.style))
            .map(|target| PathBuf::from(target.as_str()))
    }
}

/// Workspace root plus the home directory used for `~` expansion.
///
/// `root` should already be canonical; it is canonicalized again through
/// [`FsFacts`] at evaluation time so `/tmp` vs `/private/tmp` style aliases
/// cannot make an inside path look outside (or the reverse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    root: PathBuf,
    home: Option<PathBuf>,
    style: PathStyle,
}

impl Sandbox {
    /// Sandbox using the host's path syntax.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            root: root.into(),
            home,
            style: PathStyle::host(),
        }
    }

    /// Override the path syntax (tests, previews of another OS).
    #[must_use]
    pub fn with_style(mut self, style: PathStyle) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    #[must_use]
    pub fn style(&self) -> PathStyle {
        self.style
    }

    /// Bind filesystem facts and case sensitivity for a run of checks.
    #[must_use]
    pub fn checker<'a>(&'a self, case_insensitive: bool, fs: &'a dyn FsFacts) -> Checker<'a> {
        Checker::new(self, case_insensitive, fs)
    }
}

/// Result of checking one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCheck {
    pub zone: Zone,
    /// Canonical path the broker must operate on (re-checked at open time).
    /// `None` when the input could not be resolved.
    pub resolved: Option<PathBuf>,
    /// Why the path is sensitive or unresolvable, for the event log.
    pub note: Option<&'static str>,
}

/// Sensitive locations relative to `$HOME` (`/`-separated; parsed per style).
pub const SENSITIVE_IN_HOME: &[&str] = &[
    ".ssh",
    ".aws",
    ".azure",
    ".gnupg",
    ".kube",
    ".docker",
    ".config",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".gitconfig",
    ".password-store",
    ".vault-token",
    ".terraform.d",
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".local/share/keyrings",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".zlogin",
    "Library/Keychains",
    "Library/LaunchAgents",
    "Library/Cookies",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Local/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Protect",
    "AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup",
];

/// Directory names that are sensitive wherever they appear, so the check
/// still works when `$HOME` is unknown or a second home is in play.
pub const SENSITIVE_ANYWHERE: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".password-store",
    ".netrc",
    ".git-credentials",
    "Keychains",
];

/// Sensitive absolute prefixes on POSIX systems.
pub const SENSITIVE_POSIX: &[&str] = &[
    "/etc",
    "/private/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/boot",
    "/dev",
    "/proc",
    "/sys",
    "/root",
    "/System",
    "/Library/Keychains",
    "/Library/LaunchAgents",
    "/Library/LaunchDaemons",
    "/var/root",
    "/private/var/root",
    "/var/db",
    "/private/var/db",
];

/// Sensitive locations on every Windows drive (drive-relative).
pub const SENSITIVE_WINDOWS: &[&str] = &[
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "System Volume Information",
];

/// Lexically normalize `.`/`..` using the host syntax. `..` above the root
/// clamps at the root.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let style = PathStyle::host();
    let text = path.to_string_lossy();
    match parse(&text, style) {
        Ok(Parsed::Absolute(abs, _)) => PathBuf::from(abs.lexical().render(style)),
        _ => path.to_path_buf(),
    }
}

/// Whether a file name holds secrets (`.env`, `.env.local`, `.envrc`).
#[must_use]
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env" || lower.starts_with(".env.") || lower == ".envrc"
}

/// Whether the last component of `path` is a secret file.
#[must_use]
pub fn is_secret_file(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.rsplit(['/', '\\']).next().is_some_and(is_secret_name)
}

// ---------------------------------------------------------------------------
// Parsing

#[derive(Debug, Clone, PartialEq, Eq)]
enum Prefix {
    /// POSIX `/`.
    Root,
    /// Windows drive, upper-cased.
    Drive(char),
    /// Windows UNC share, lower-cased for comparison.
    Unc { server: String, share: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbsPath {
    prefix: Prefix,
    parts: Vec<String>,
}

impl AbsPath {
    fn render(&self, style: PathStyle) -> String {
        match (&self.prefix, style) {
            (Prefix::Root, _) => format!("/{}", self.parts.join("/")),
            (Prefix::Drive(letter), _) => format!("{letter}:\\{}", self.parts.join("\\")),
            (Prefix::Unc { server, share }, _) => {
                let mut out = format!("\\\\{server}\\{share}");
                for part in &self.parts {
                    out.push('\\');
                    out.push_str(part);
                }
                out
            }
        }
    }

    fn parent(&self) -> Option<Self> {
        if self.parts.is_empty() {
            return None;
        }
        let mut parent = self.clone();
        parent.parts.pop();
        Some(parent)
    }

    fn lexical(&self) -> Self {
        let mut out = Self {
            prefix: self.prefix.clone(),
            parts: Vec::new(),
        };
        for part in &self.parts {
            if part == ".." {
                out.parts.pop();
            } else {
                out.parts.push(part.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Parsed {
    /// Absolute path; the flag marks a Windows verbatim (`\\?\`) path.
    Absolute(AbsPath, bool),
    Relative(Vec<String>),
    /// Windows `\foo`: absolute on the current drive.
    RootRelative(Vec<String>),
}

fn parse(text: &str, style: PathStyle) -> Result<Parsed, &'static str> {
    if text.contains('\0') {
        return Err("path contains a NUL byte");
    }
    match style {
        PathStyle::Posix => Ok(if let Some(rest) = text.strip_prefix('/') {
            Parsed::Absolute(
                AbsPath {
                    prefix: Prefix::Root,
                    parts: posix_parts(rest),
                },
                false,
            )
        } else {
            Parsed::Relative(posix_parts(text))
        }),
        PathStyle::Windows => parse_windows(text),
    }
}

fn posix_parts(text: &str) -> Vec<String> {
    text.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_string)
        .collect()
}

fn parse_windows(text: &str) -> Result<Parsed, &'static str> {
    let chars: Vec<char> = text.chars().collect();
    let sep = |index: usize| chars.get(index).is_some_and(|c| *c == '\\' || *c == '/');

    // Verbatim / NT object prefixes: `\\?\` and `\??\`.
    if text.starts_with("\\\\?\\") || text.starts_with("\\??\\") {
        let rest = &text[4..];
        if rest.contains('/') {
            return Err("forward slash inside a verbatim path");
        }
        let lower = rest.to_ascii_lowercase();
        if let Some(unc) = lower.strip_prefix("unc\\") {
            let _ = unc;
            let after = &rest[4..];
            return unc_path(after, true);
        }
        let rest_chars: Vec<char> = rest.chars().collect();
        if rest_chars.len() >= 2 && rest_chars[0].is_ascii_alphabetic() && rest_chars[1] == ':' {
            if rest_chars.len() > 2 && rest_chars[2] != '\\' {
                return Err("malformed verbatim drive path");
            }
            let parts = verbatim_parts(rest.get(3..).unwrap_or(""))?;
            return Ok(Parsed::Absolute(
                AbsPath {
                    prefix: Prefix::Drive(rest_chars[0].to_ascii_uppercase()),
                    parts,
                },
                true,
            ));
        }
        return Err("device or volume path");
    }
    // Device namespace: `\\.\PhysicalDrive0`, `//./pipe/x`.
    if sep(0) && sep(1) && chars.get(2) == Some(&'.') && (chars.len() == 3 || sep(3)) {
        return Err("device path");
    }
    if sep(0) && sep(1) {
        return unc_path(&text[2..], false);
    }
    if chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':' {
        if !sep(2) {
            return Err("drive-relative path");
        }
        return Ok(Parsed::Absolute(
            AbsPath {
                prefix: Prefix::Drive(chars[0].to_ascii_uppercase()),
                parts: windows_parts(&text[3..])?,
            },
            false,
        ));
    }
    if sep(0) {
        return Ok(Parsed::RootRelative(windows_parts(&text[1..])?));
    }
    Ok(Parsed::Relative(windows_parts(text)?))
}

fn unc_path(rest: &str, verbatim: bool) -> Result<Parsed, &'static str> {
    let mut pieces = rest.split(['\\', '/']);
    let server = pieces.next().unwrap_or("").to_ascii_lowercase();
    let share = pieces.next().unwrap_or("").to_ascii_lowercase();
    if server.is_empty() || share.is_empty() {
        return Err("incomplete UNC path");
    }
    let remainder: Vec<&str> = pieces.collect();
    let remainder = remainder.join("\\");
    let parts = if verbatim {
        verbatim_parts(&remainder)?
    } else {
        windows_parts(&remainder)?
    };
    // `\\localhost\C$\…` is the local C: drive through the admin share.
    let local = matches!(
        server.as_str(),
        "localhost" | "127.0.0.1" | "[::1]" | "::1" | "0:0:0:0:0:0:0:1"
    ) || server.starts_with("127.");
    let share_chars: Vec<char> = share.chars().collect();
    if local
        && share_chars.len() == 2
        && share_chars[0].is_ascii_alphabetic()
        && share_chars[1] == '$'
    {
        return Ok(Parsed::Absolute(
            AbsPath {
                prefix: Prefix::Drive(share_chars[0].to_ascii_uppercase()),
                parts,
            },
            verbatim,
        ));
    }
    Ok(Parsed::Absolute(
        AbsPath {
            prefix: Prefix::Unc { server, share },
            parts,
        },
        verbatim,
    ))
}

const RESERVED_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9",
];

/// Win32 path cleanup: trailing dots and spaces are dropped and `name:stream`
/// addresses `name`. Components that only make sense to the device layer are
/// refused instead of guessed.
fn windows_parts(text: &str) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    for raw in text.split(['\\', '/']) {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            out.push(raw.to_string());
            continue;
        }
        let without_stream = raw.split(':').next().unwrap_or("");
        let cleaned = without_stream.trim_end_matches(['.', ' ']);
        if cleaned.is_empty() {
            return Err("ambiguous path component");
        }
        let stem = cleaned
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_lowercase();
        if RESERVED_DEVICE_NAMES.contains(&stem.as_str()) {
            return Err("reserved device name");
        }
        out.push(cleaned.to_string());
    }
    Ok(out)
}

/// Verbatim paths skip Win32 cleanup, so `.`/`..` are literal names there.
/// Refuse them rather than guess which interpretation a caller will use.
fn verbatim_parts(text: &str) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    for raw in text.split('\\') {
        if raw.is_empty() {
            continue;
        }
        if raw == "." || raw == ".." {
            return Err("dot component inside a verbatim path");
        }
        if raw.contains(':') {
            return Err("stream or device syntax inside a verbatim path");
        }
        out.push(raw.to_string());
    }
    Ok(out)
}

fn join_raw(base: &AbsPath, input: &str, style: PathStyle) -> Option<AbsPath> {
    match parse(input, style).ok()? {
        Parsed::Absolute(abs, _) => Some(abs),
        Parsed::Relative(parts) => {
            let mut out = base.clone();
            out.parts.extend(parts);
            Some(out)
        }
        Parsed::RootRelative(parts) => Some(AbsPath {
            prefix: base.prefix.clone(),
            parts,
        }),
    }
}

fn parse_absolute(path: &Path, style: PathStyle) -> Option<AbsPath> {
    match parse(&path.to_string_lossy(), style).ok()? {
        Parsed::Absolute(abs, _) => Some(abs.lexical()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Checking

/// A sandbox bound to filesystem facts and a case policy.
pub struct Checker<'a> {
    sandbox: &'a Sandbox,
    fs: &'a dyn FsFacts,
    case_insensitive: bool,
    /// Canonical root, or `None` if the configured root is unusable.
    root: Option<AbsPath>,
    /// Home as configured and as canonicalized.
    homes: Vec<AbsPath>,
}

impl<'a> Checker<'a> {
    fn new(sandbox: &'a Sandbox, case_insensitive: bool, fs: &'a dyn FsFacts) -> Self {
        let style = sandbox.style;
        let canonical = |path: &Path| -> Option<AbsPath> {
            let raw = parse_absolute(path, style)?;
            let rendered = PathBuf::from(raw.render(style));
            Some(
                fs.canonicalize(&rendered)
                    .and_then(|real| parse_absolute(&real, style))
                    .unwrap_or(raw),
            )
        };
        // A root with no components (`/`, `C:\`) would contain everything, so
        // it is treated as containing nothing.
        let root = canonical(&sandbox.root).filter(|root| !root.parts.is_empty());
        let mut homes = Vec::new();
        if let Some(home) = &sandbox.home {
            if let Some(raw) = parse_absolute(home, style) {
                homes.push(raw);
            }
            if let Some(real) = canonical(home) {
                if !homes.contains(&real) {
                    homes.push(real);
                }
            }
        }
        Self {
            sandbox,
            fs,
            case_insensitive: case_insensitive || style == PathStyle::Windows,
            root,
            homes,
        }
    }

    fn style(&self) -> PathStyle {
        self.sandbox.style
    }

    /// Canonical workspace root, if usable.
    #[must_use]
    pub fn root_path(&self) -> Option<PathBuf> {
        self.root
            .as_ref()
            .map(|root| PathBuf::from(root.render(self.style())))
    }

    /// Check a path as a file tool would use it: no glob or variable expansion.
    /// Relative paths resolve against `cwd` (default: the workspace root).
    #[must_use]
    pub fn check(&self, cwd: Option<&Path>, input: &str) -> PathCheck {
        match self.resolve(cwd, input) {
            Ok((lexical, canonical)) => {
                let (zone, note) = self.zone_of(&lexical, &canonical);
                PathCheck {
                    zone,
                    resolved: Some(PathBuf::from(canonical.render(self.style()))),
                    note,
                }
            }
            Err(note) => PathCheck {
                zone: Zone::Sensitive,
                resolved: None,
                note: Some(note),
            },
        }
    }

    /// Check a path that a shell will glob-expand. Glob characters outside the
    /// workspace, or followed by `..`, make the target unknowable.
    #[must_use]
    pub fn check_shell_word(&self, cwd: Option<&Path>, input: &str) -> PathCheck {
        let check = self.check(cwd, input);
        let has_glob = input.contains(['*', '?', '[']);
        if !has_glob {
            return check;
        }
        let dotdot_after_glob = input
            .split(['/', '\\'])
            .skip_while(|part| !part.contains(['*', '?', '[']))
            .any(|part| part == "..");
        if check.zone != Zone::Inside || dotdot_after_glob {
            return PathCheck {
                zone: Zone::Sensitive,
                note: Some("glob pattern outside the workspace"),
                ..check
            };
        }
        check
    }

    /// Whether `path` (already canonical) is a secret or repository-control
    /// file that `auto` must not touch silently.
    #[must_use]
    pub fn is_protected(&self, path: &Path) -> bool {
        let Some(abs) = parse_absolute(path, self.style()) else {
            return true;
        };
        let parts = match &self.root {
            Some(root) if self.within(&abs, root) => &abs.parts[root.parts.len()..],
            _ => &abs.parts[..],
        };
        parts.iter().any(|part| {
            let lower = part.to_ascii_lowercase();
            lower == ".git" || lower == ".next-agent"
        }) || parts.last().is_some_and(|name| is_secret_name(name))
    }

    /// Whether `path` names a secret file (`.env*`), which `auto` must not
    /// read silently either.
    #[must_use]
    pub fn is_secret(&self, path: &Path) -> bool {
        is_secret_file(path)
    }

    fn resolve(&self, cwd: Option<&Path>, input: &str) -> Result<(AbsPath, AbsPath), &'static str> {
        let style = self.style();
        let expanded = self.expand_home(input)?;
        if expanded.starts_with('$') || (style == PathStyle::Windows && expanded.starts_with('%')) {
            return Err("unexpanded variable in path");
        }
        let base = match cwd {
            Some(dir) => {
                let text = dir.to_string_lossy();
                let (_, canonical) = self.resolve_from(self.root.clone(), &text)?;
                Some(canonical)
            }
            None => self.root.clone(),
        };
        self.resolve_from(base, &expanded)
    }

    fn expand_home(&self, input: &str) -> Result<String, &'static str> {
        let style = self.style();
        let Some(rest) = input.strip_prefix('~') else {
            return Ok(input.to_string());
        };
        if !(rest.is_empty() || rest.starts_with(|c| style.is_sep(c))) {
            return Err("~user expansion is not supported");
        }
        let home = self.homes.first().ok_or("no home directory configured")?;
        Ok(format!("{}{rest}", home.render(style)))
    }

    fn resolve_from(
        &self,
        base: Option<AbsPath>,
        input: &str,
    ) -> Result<(AbsPath, AbsPath), &'static str> {
        let style = self.style();
        let raw = match parse(input, style)? {
            Parsed::Absolute(abs, _) => abs,
            Parsed::Relative(parts) => {
                let mut abs = base.ok_or("relative path without a usable workspace root")?;
                abs.parts.extend(parts);
                abs
            }
            Parsed::RootRelative(parts) => {
                let base = base.ok_or("root-relative path without a usable workspace root")?;
                AbsPath {
                    prefix: base.prefix,
                    parts,
                }
            }
        };
        let lexical = raw.lexical();

        // Physical walk: canonicalize after each component so `link/..` goes
        // to the link target's parent, the way the kernel resolves it.
        let mut acc = AbsPath {
            prefix: raw.prefix.clone(),
            parts: Vec::new(),
        };
        let mut exists = true;
        for part in &raw.parts {
            if part == ".." {
                acc.parts.pop();
                continue;
            }
            acc.parts.push(part.clone());
            if !exists {
                continue;
            }
            let rendered = PathBuf::from(acc.render(style));
            if let Some(real) = self.fs.canonicalize(&rendered) {
                acc = parse_absolute(&real, style).ok_or("unparseable canonical path")?;
            } else if self.fs.read_link(&rendered).is_some() {
                return Err("dangling symlink");
            } else {
                exists = false;
            }
        }
        Ok((lexical, acc))
    }

    fn zone_of(&self, lexical: &AbsPath, canonical: &AbsPath) -> (Zone, Option<&'static str>) {
        if let Some(note) = self
            .sensitive(lexical)
            .or_else(|| self.sensitive(canonical))
        {
            return (Zone::Sensitive, Some(note));
        }
        if self.style() == PathStyle::Windows && canonical.parts.iter().any(|p| is_short_name(p)) {
            return (Zone::Sensitive, Some("unresolved 8.3 short name"));
        }
        match &self.root {
            Some(root) if self.within(canonical, root) => (Zone::Inside, None),
            _ => (Zone::Outside, None),
        }
    }

    fn sensitive(&self, path: &AbsPath) -> Option<&'static str> {
        let style = self.style();
        if path.parts.iter().any(|part| {
            SENSITIVE_ANYWHERE
                .iter()
                .any(|name| self.eq_part(part, name))
        }) {
            return Some("credential directory");
        }
        for home in &self.homes {
            for entry in SENSITIVE_IN_HOME {
                let mut prefix = home.clone();
                prefix.parts.extend(entry.split('/').map(str::to_string));
                if self.within(path, &prefix) {
                    return Some("sensitive location in home");
                }
            }
        }
        match (style, &path.prefix) {
            (PathStyle::Posix, Prefix::Root) => SENSITIVE_POSIX
                .iter()
                .filter_map(|entry| parse_absolute(Path::new(entry), style))
                .any(|prefix| self.within(path, &prefix))
                .then_some("system location"),
            (PathStyle::Windows, Prefix::Drive(letter)) => SENSITIVE_WINDOWS
                .iter()
                .any(|entry| {
                    let prefix = AbsPath {
                        prefix: Prefix::Drive(*letter),
                        parts: vec![(*entry).to_string()],
                    };
                    self.within(path, &prefix)
                })
                .then_some("system location"),
            _ => None,
        }
    }

    fn within(&self, child: &AbsPath, parent: &AbsPath) -> bool {
        child.prefix == parent.prefix
            && parent.parts.len() <= child.parts.len()
            && child
                .parts
                .iter()
                .zip(&parent.parts)
                .all(|(a, b)| self.eq_part(a, b))
    }

    fn eq_part(&self, a: &str, b: &str) -> bool {
        if self.case_insensitive {
            fold(a) == fold(b)
        } else {
            a == b
        }
    }
}

/// Case-fold for comparison and drop default-ignorable code points, which
/// HFS+ ignores in names (`.s\u{200c}sh` opens `.ssh`).
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| !is_ignorable(*c))
        .collect::<String>()
        .to_lowercase()
}

fn is_ignorable(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{206A}'..='\u{206F}'
            | '\u{FEFF}'
    )
}

/// `PROGRA~1`, `NEWFIL~1.TXT`: DOS 8.3 aliases that only the filesystem can
/// expand.
fn is_short_name(part: &str) -> bool {
    let chars: Vec<char> = part.chars().collect();
    chars
        .windows(2)
        .any(|pair| pair[0] == '~' && pair[1].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posix() -> Sandbox {
        Sandbox::new("/work/project", Some(PathBuf::from("/home/dev"))).with_style(PathStyle::Posix)
    }

    fn windows() -> Sandbox {
        Sandbox::new("C:\\work\\project", Some(PathBuf::from("C:\\Users\\dev")))
            .with_style(PathStyle::Windows)
    }

    fn zone_with(sandbox: &Sandbox, ci: bool, fs: &dyn FsFacts, input: &str) -> Zone {
        sandbox.checker(ci, fs).check(None, input).zone
    }

    fn zone(sandbox: &Sandbox, input: &str) -> Zone {
        zone_with(sandbox, false, &Lexical, input)
    }

    #[test]
    fn normalizes_dot_and_dotdot() {
        if PathStyle::host() == PathStyle::Posix {
            assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
            assert_eq!(normalize(Path::new("/a/../../b")), PathBuf::from("/b"));
            assert_eq!(normalize(Path::new("//a//b")), PathBuf::from("/a/b"));
        }
    }

    #[test]
    fn resolves_relative_and_home() {
        let sandbox = posix();
        let checker = sandbox.checker(false, &Lexical);
        assert_eq!(
            checker.check(None, "src/a.rs").resolved,
            Some(PathBuf::from("/work/project/src/a.rs"))
        );
        assert_eq!(
            checker.check(None, "~/k").resolved,
            Some(PathBuf::from("/home/dev/k"))
        );
        assert_eq!(
            checker.check(Some(Path::new("src")), "../b").resolved,
            Some(PathBuf::from("/work/project/b"))
        );
    }

    #[test]
    fn containment_is_component_wise() {
        let sandbox = posix();
        assert_eq!(zone(&sandbox, "/work/project/src"), Zone::Inside);
        assert_eq!(zone(&sandbox, "/work/project-evil/x"), Zone::Outside);
        assert_eq!(zone(&sandbox, "/work"), Zone::Outside);
    }

    #[test]
    fn root_of_filesystem_contains_nothing() {
        let sandbox = Sandbox::new("/", None).with_style(PathStyle::Posix);
        assert_eq!(zone(&sandbox, "/anything"), Zone::Outside);
        let relative = Sandbox::new("relative/root", None).with_style(PathStyle::Posix);
        assert_eq!(zone(&relative, "x"), Zone::Sensitive);
    }

    #[test]
    fn symlink_escape_is_outside() {
        let sandbox = posix();
        let fs = MapFs::new(PathStyle::Posix)
            .with_entry("/work/project/src/lib.rs")
            .with_link("/work/project/out", "/tmp/elsewhere")
            .with_entry("/tmp/elsewhere")
            .with_link("/work/project/keys", "/home/dev/.ssh")
            .with_entry("/home/dev/.ssh/id_rsa");
        assert_eq!(zone_with(&sandbox, false, &fs, "out/x"), Zone::Outside);
        assert_eq!(
            zone_with(&sandbox, false, &fs, "keys/id_rsa"),
            Zone::Sensitive
        );
        assert_eq!(zone_with(&sandbox, false, &fs, "src/lib.rs"), Zone::Inside);
    }

    #[test]
    fn dotdot_through_symlink_follows_the_kernel() {
        // `link/..` is the parent of the link *target*, not the workspace.
        let sandbox = posix();
        let fs = MapFs::new(PathStyle::Posix)
            .with_entry("/work/project")
            .with_link("/work/project/link", "/etc/ssh")
            .with_entry("/etc/ssh/sshd_config")
            .with_entry("/etc/passwd");
        let check = sandbox.checker(false, &fs).check(None, "link/../passwd");
        assert_eq!(check.zone, Zone::Sensitive);
        assert_eq!(check.resolved, Some(PathBuf::from("/etc/passwd")));
    }

    #[test]
    fn dangling_symlink_is_refused() {
        let sandbox = posix();
        let fs = MapFs::new(PathStyle::Posix)
            .with_entry("/work/project")
            .with_link("/work/project/new", "/home/dev/.bashrc_new");
        assert_eq!(zone_with(&sandbox, false, &fs, "new"), Zone::Sensitive);
    }

    #[test]
    fn windows_variants() {
        let sandbox = windows();
        assert_eq!(zone(&sandbox, "src\\a.rs"), Zone::Inside);
        assert_eq!(zone(&sandbox, "C:/work/project/a"), Zone::Inside);
        assert_eq!(zone(&sandbox, "c:\\WORK\\Project\\a"), Zone::Inside);
        assert_eq!(zone(&sandbox, "\\\\?\\C:\\work\\project\\a"), Zone::Inside);
        assert_eq!(zone(&sandbox, "C:\\Windows.\\System32"), Zone::Sensitive);
        assert_eq!(
            zone(&sandbox, "\\\\localhost\\c$\\Windows"),
            Zone::Sensitive
        );
        assert_eq!(zone(&sandbox, "\\\\.\\PhysicalDrive0"), Zone::Sensitive);
        assert_eq!(zone(&sandbox, "\\\\server\\share\\x"), Zone::Outside);
        assert_eq!(zone(&sandbox, "C:foo"), Zone::Sensitive);
        assert_eq!(zone(&sandbox, "D:\\Program Files\\x"), Zone::Sensitive);
        assert_eq!(zone(&sandbox, "src\\NUL.txt"), Zone::Sensitive);
    }

    #[test]
    fn case_and_ignorables_on_case_insensitive_volumes() {
        let sandbox = posix();
        assert_eq!(
            zone_with(&sandbox, true, &Lexical, "/WORK/project/a"),
            Zone::Inside
        );
        assert_eq!(
            zone_with(&sandbox, false, &Lexical, "/WORK/project/a"),
            Zone::Outside
        );
        assert_eq!(
            zone_with(&sandbox, true, &Lexical, "/home/dev/.S\u{200C}SH/id_rsa"),
            Zone::Sensitive
        );
    }

    #[test]
    fn protected_and_secret_files() {
        let sandbox = posix();
        let checker = sandbox.checker(false, &Lexical);
        assert!(checker.is_protected(Path::new("/work/project/.env")));
        assert!(checker.is_protected(Path::new("/work/project/.git/hooks/pre-commit")));
        assert!(checker.is_protected(Path::new("/work/project/.next-agent/state")));
        assert!(!checker.is_protected(Path::new("/work/project/src/env.rs")));
        assert!(is_secret_file(Path::new("/w/.env.local")));
        assert!(!is_secret_file(Path::new("/w/env.txt")));
    }

    #[test]
    fn shell_globs_outside_are_unknowable() {
        let sandbox = posix();
        let checker = sandbox.checker(false, &Lexical);
        assert_eq!(
            checker.check_shell_word(None, "src/*.rs").zone,
            Zone::Inside
        );
        assert_eq!(
            checker.check_shell_word(None, "~/.ss?/id_rsa").zone,
            Zone::Sensitive
        );
        assert_eq!(
            checker.check_shell_word(None, "src/*/../../x").zone,
            Zone::Sensitive
        );
    }
}
