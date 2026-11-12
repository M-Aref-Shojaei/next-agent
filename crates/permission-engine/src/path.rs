//! Lexical path sandbox: normalize, expand `~`, and classify a target as
//! inside the workspace, outside it, or inside a sensitive directory.
//!
//! This module is deliberately I/O-free. Symlink resolution needs the
//! filesystem, so the broker canonicalizes the nearest existing ancestor and
//! then calls [`zone`] on the result. Keeping the decision pure means the
//! whole boundary corpus is table-testable.

use std::path::{Component, Path, PathBuf};

/// Where a resolved path sits relative to the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Inside the workspace root (or active worktree).
    Inside,
    /// Outside the workspace and not on the sensitive list.
    Outside,
    /// Outside the workspace and on the sensitive list (keys, credentials, `/etc`).
    Sensitive,
}

/// Workspace root plus the home directory used for `~` expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    root: PathBuf,
    home: Option<PathBuf>,
}

impl Sandbox {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            root: normalize(&root.into()),
            home: home.map(|path| normalize(&path)),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Resolve a request path against `cwd` and the home directory.
    #[must_use]
    pub fn resolve(&self, cwd: &Path, input: &Path) -> PathBuf {
        expand(input, cwd, self.home())
    }
}

/// Sensitive directories relative to `$HOME`.
pub const SENSITIVE_IN_HOME: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".docker",
    ".config",
    "Library/Keychains",
    "AppData/Roaming/Microsoft/Credentials",
];

/// Sensitive absolute prefixes.
pub const SENSITIVE_ABSOLUTE: &[&str] = &[
    "/etc",
    "/usr",
    "/System",
    "/bin",
    "/sbin",
    "/var/root",
    "C:\\Windows",
    "C:\\Program Files",
];

/// Lexically normalize `.`/`..` without touching the filesystem.
///
/// `..` above the root collapses to the root, which is what shells do for
/// absolute paths and prevents `../../..` from escaping by accident.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Expand a leading `~` or `~/`, join relative paths to `cwd`, then normalize.
#[must_use]
pub fn expand(input: &Path, cwd: &Path, home: Option<&Path>) -> PathBuf {
    let text = input.to_string_lossy();
    let expanded = if text == "~" {
        home.map_or_else(|| input.to_path_buf(), Path::to_path_buf)
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.map_or_else(|| input.to_path_buf(), |dir| dir.join(rest))
    } else {
        input.to_path_buf()
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    normalize(&absolute)
}

/// Component-wise containment. String prefix checks would accept `/work-evil`
/// for root `/work`, so components are compared instead.
#[must_use]
pub fn is_within(child: &Path, parent: &Path, case_insensitive: bool) -> bool {
    let child: Vec<String> = components(child, case_insensitive);
    let parent: Vec<String> = components(parent, case_insensitive);
    if parent.is_empty() || parent.len() > child.len() {
        return false;
    }
    child.iter().zip(&parent).all(|(a, b)| a == b)
}

fn components(path: &Path, case_insensitive: bool) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy().into_owned();
            if case_insensitive {
                text.to_lowercase()
            } else {
                text
            }
        })
        .collect()
}

/// Whether a normalized absolute path is on the sensitive list.
#[must_use]
pub fn is_sensitive(target: &Path, sandbox: &Sandbox, case_insensitive: bool) -> bool {
    if let Some(home) = sandbox.home() {
        for entry in SENSITIVE_IN_HOME {
            if is_within(target, &home.join(entry), case_insensitive) {
                return true;
            }
        }
    }
    SENSITIVE_ABSOLUTE.iter().any(|prefix| {
        let prefix = Path::new(prefix);
        is_within(target, prefix, case_insensitive) || paths_equal(target, prefix, case_insensitive)
    })
}

fn paths_equal(a: &Path, b: &Path, case_insensitive: bool) -> bool {
    components(a, case_insensitive) == components(b, case_insensitive)
}

/// Classify an already-resolved absolute path.
#[must_use]
pub fn zone(target: &Path, sandbox: &Sandbox, case_insensitive: bool) -> Zone {
    if is_sensitive(target, sandbox, case_insensitive) {
        Zone::Sensitive
    } else if is_within(target, sandbox.root(), case_insensitive) {
        Zone::Inside
    } else {
        Zone::Outside
    }
}

/// Extra check: a write to a dotenv file inside the workspace still asks in
/// `auto`, because it usually holds live credentials.
#[must_use]
pub fn is_secret_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".env" || name.starts_with(".env."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> Sandbox {
        Sandbox::new("/work/project", Some(PathBuf::from("/home/dev")))
    }

    #[test]
    fn normalizes_dot_and_dotdot() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/a/../../b")), PathBuf::from("/b"));
        assert_eq!(normalize(Path::new("//a//b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn expands_tilde_and_relative_paths() {
        let box_ = sandbox();
        assert_eq!(
            box_.resolve(Path::new("/x"), Path::new("~/k")),
            PathBuf::from("/home/dev/k")
        );
        assert_eq!(
            box_.resolve(Path::new("/x"), Path::new("~")),
            PathBuf::from("/home/dev")
        );
        assert_eq!(
            box_.resolve(box_.root(), Path::new("src/a.rs")),
            PathBuf::from("/work/project/src/a.rs")
        );
    }

    #[test]
    fn containment_is_component_wise() {
        assert!(is_within(
            Path::new("/work/project/src"),
            Path::new("/work/project"),
            false
        ));
        assert!(!is_within(
            Path::new("/work/project-evil"),
            Path::new("/work/project"),
            false
        ));
        assert!(!is_within(
            Path::new("/work"),
            Path::new("/work/project"),
            false
        ));
    }

    /// The escape corpus: every one of these must resolve outside the root.
    #[test]
    fn escape_corpus_resolves_outside() {
        let box_ = sandbox();
        let cwd = box_.root.clone();
        let escapes = [
            "../",
            "..",
            "../..",
            "../../../../etc/passwd",
            "src/../../etc/passwd",
            "/etc/passwd",
            "~/",
            "~/.ssh/id_ed25519",
            "./../../out.txt",
            "a/../../b",
            "/work/project/../other",
            "/work/project-evil/x",
            "/work",
            "/",
            "/tmp/../var/root/secret",
        ];
        for input in escapes {
            let resolved = box_.resolve(&cwd, Path::new(input));
            assert_ne!(
                zone(&resolved, &box_, false),
                Zone::Inside,
                "{input} resolved to {resolved:?} but was treated as inside"
            );
        }
    }

    #[test]
    fn inside_corpus_stays_inside() {
        let box_ = sandbox();
        let cwd = box_.root.clone();
        for input in [
            ".",
            "src",
            "src/lib.rs",
            "./a/./b",
            "a/../b",
            "/work/project/src/a.rs",
        ] {
            let resolved = box_.resolve(&cwd, Path::new(input));
            assert_eq!(
                zone(&resolved, &box_, false),
                Zone::Inside,
                "{input} resolved to {resolved:?}"
            );
        }
    }

    #[test]
    fn sensitive_paths_are_flagged() {
        let box_ = sandbox();
        assert_eq!(
            zone(Path::new("/home/dev/.ssh/id_rsa"), &box_, false),
            Zone::Sensitive
        );
        assert_eq!(
            zone(Path::new("/home/dev/.aws/credentials"), &box_, false),
            Zone::Sensitive
        );
        assert_eq!(zone(Path::new("/etc/hosts"), &box_, false), Zone::Sensitive);
        assert_eq!(
            zone(Path::new("/home/dev/notes.md"), &box_, false),
            Zone::Outside
        );
        assert_eq!(
            zone(Path::new("/work/project/README.md"), &box_, false),
            Zone::Inside
        );
    }

    #[test]
    fn case_insensitive_comparison_for_macos_and_windows() {
        let _box_ = sandbox();
        assert!(is_within(
            Path::new("/Work/Project/A"),
            Path::new("/work/project"),
            true
        ));
        assert!(!is_within(
            Path::new("/Work/Project/A"),
            Path::new("/work/project"),
            false
        ));
    }

    #[test]
    fn secret_files_are_recognised() {
        assert!(is_secret_file(Path::new("/w/.env")));
        assert!(is_secret_file(Path::new("/w/.env.local")));
        assert!(!is_secret_file(Path::new("/w/env.txt")));
    }
}
