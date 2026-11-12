//! Workspace registration: canonical paths and stable ids.

use std::path::{Component, Path, PathBuf};

/// A project folder the agent may operate on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    id: String,
    root: PathBuf,
    name: String,
}

/// Reasons a workspace cannot be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    EmptyPath,
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "workspace path does not exist: {}", p.display()),
            Self::NotADirectory(p) => {
                write!(f, "workspace path is not a directory: {}", p.display())
            }
            Self::EmptyPath => write!(f, "workspace path is empty"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl Workspace {
    /// Open a workspace, canonicalizing its root (resolves symlinks and `..`).
    ///
    /// # Errors
    /// Returns [`WorkspaceError`] if the path is empty, missing, or not a directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(WorkspaceError::EmptyPath);
        }
        if !path.exists() {
            return Err(WorkspaceError::NotFound(path.to_path_buf()));
        }
        if !path.is_dir() {
            return Err(WorkspaceError::NotADirectory(path.to_path_buf()));
        }
        let root = path
            .canonicalize()
            .map_err(|_| WorkspaceError::NotFound(path.to_path_buf()))?;
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        Ok(Self {
            id: stable_id(&root),
            root,
            name,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether `candidate` lies inside the workspace root (lexically, after
    /// canonicalizing the nearest existing ancestor).
    #[must_use]
    pub fn contains(&self, candidate: impl AsRef<Path>) -> bool {
        let Ok(path) = canonicalize_lenient(candidate.as_ref()) else {
            return false;
        };
        path.starts_with(&self.root)
    }
}

/// Resolve `.` and `..` purely lexically, keeping the root/prefix fixed.
#[must_use]
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut prefix: Option<PathBuf> = None;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                prefix = Some(PathBuf::from(component.as_os_str()));
            }
            Component::RootDir => match prefix.as_mut() {
                Some(drive) => drive.push(component.as_os_str()),
                None => prefix = Some(PathBuf::from(component.as_os_str())),
            },
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() && prefix.is_none() {
                    stack.push("..".into());
                }
            }
            Component::Normal(name) => stack.push(name.to_os_string()),
        }
    }

    let mut out = prefix.unwrap_or_default();
    for name in stack {
        out.push(name);
    }
    out
}

/// Canonicalize a path that may not exist yet by canonicalizing the nearest
/// existing ancestor and re-appending the remainder.
pub fn canonicalize_lenient(path: &Path) -> std::io::Result<PathBuf> {
    let path = normalize_lexical(path);
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }

    let mut remainder = Vec::new();
    let mut current = path;
    loop {
        if let Ok(base) = current.canonicalize() {
            let mut result = base;
            for component in remainder.iter().rev() {
                result.push(component);
            }
            return Ok(result);
        }
        if let Some(name) = current.file_name() {
            remainder.push(name.to_os_string());
            if !current.pop() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no existing ancestor",
                ));
            }
        } else {
            if current.pop() {
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no existing ancestor",
            ));
        }
    }
}

/// Deterministic, path-derived id: `ws_` + FNV-1a 64 hex.
#[must_use]
pub fn stable_id(root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("ws_{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("na-core-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn opens_existing_directory() {
        let dir = temp_dir("open");
        let ws = Workspace::open(&dir).expect("open");
        assert_eq!(ws.root(), dir.canonicalize().expect("canon"));
        assert!(ws.id().starts_with("ws_"));
        assert_eq!(ws.id().len(), 19);
    }

    #[test]
    fn rejects_missing_and_file_paths() {
        let dir = temp_dir("reject");
        assert_eq!(
            Workspace::open(dir.join("nope")),
            Err(WorkspaceError::NotFound(dir.join("nope")))
        );
        let file = dir.join("file.txt");
        fs::write(&file, "x").expect("write");
        assert_eq!(
            Workspace::open(&file),
            Err(WorkspaceError::NotADirectory(file))
        );
        assert_eq!(Workspace::open(""), Err(WorkspaceError::EmptyPath));
    }

    #[test]
    fn id_is_deterministic_and_path_dependent() {
        let a = temp_dir("id-a");
        let b = temp_dir("id-b");
        let wa = Workspace::open(&a).expect("a");
        let wa2 = Workspace::open(&a).expect("a2");
        let wb = Workspace::open(&b).expect("b");
        assert_eq!(wa.id(), wa2.id());
        assert_ne!(wa.id(), wb.id());
    }

    #[test]
    fn contains_only_inside_root() {
        let root = temp_dir("contains");
        let ws = Workspace::open(&root).expect("ws");
        assert!(ws.contains(&root));
        assert!(ws.contains(root.join("src/lib.rs")));
        assert!(ws.contains(root.join("a/b/../c")));
        assert!(!ws.contains(root.join("../outside")));
        assert!(!ws.contains(root.join("../../outside")));
    }

    #[cfg(unix)]
    #[test]
    fn contains_rejects_symlink_escape() {
        let root = temp_dir("symlink-root");
        let outside = temp_dir("symlink-outside");
        fs::write(outside.join("secret.txt"), "x").expect("secret");
        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        let ws = Workspace::open(&root).expect("ws");
        assert!(!ws.contains(link.join("secret.txt")));
    }

    #[test]
    fn canonicalize_lenient_handles_missing_leaf() {
        let dir = temp_dir("lenient");
        let path = dir.join("a/b/c.txt");
        let out = canonicalize_lenient(&path).expect("lenient");
        assert_eq!(out, dir.canonicalize().expect("canon").join("a/b/c.txt"));
    }

    #[test]
    fn normalize_lexical_cases() {
        assert_eq!(normalize_lexical(Path::new("/a/b/../c")), Path::new("/a/c"));
        assert_eq!(normalize_lexical(Path::new("/a/./b/")), Path::new("/a/b"));
        assert_eq!(normalize_lexical(Path::new("/../a")), Path::new("/a"));
        assert_eq!(normalize_lexical(Path::new("a/../b")), Path::new("b"));
        assert_eq!(normalize_lexical(Path::new("/a/../..")), Path::new("/"));
    }
}
