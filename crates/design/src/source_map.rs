// Adapted from Onlook (Apache-2.0, © 2024 On Off, Inc.): the idea of
// mapping every rendered element back to its place in the source
// (packages/parser, element ids → file + position) @ 423e2e9 — modified:
// re-implemented in Rust with no ids; locations come from framework dev
// metadata or our `data-na-src` build plugin, and are validated here.
//!
//! Element → source mapping for the Design Studio's edit mode.
//!
//! The preview bridge (`bridge.js`) reports where an element came from using
//! whatever the page offers, in this order:
//! 1. `data-na-src="file:line:col"` from our build plugin
//!    (`packages/design-instrument`), any framework;
//! 2. Svelte dev metadata (`element.__svelte_meta.loc`), zero config;
//! 3. React ≤ 18 dev fibers (`_debugSource`), zero config;
//! 4. Vue dev components (`__vueParentComponent.type.__file`, file only).
//!
//! The page is untrusted: everything it reports is normalised and checked to
//! name a real source file **inside the workspace** before anything reads or
//! edits it.

use crate::DesignError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Which mechanism produced a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// `data-na-src` from the build plugin (1-based line, 1-based column).
    Attr,
    /// Svelte 5 `__svelte_meta.loc` (1-based line, 0-based column).
    Svelte,
    /// Svelte 3/4 `__svelte_meta.loc` (0-based line and column).
    Svelte4,
    /// React `_debugSource` (1-based line, 1-based column).
    React,
    /// Vue `__file` (file only).
    Vue,
}

/// What the bridge sends for a clicked element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawLocation {
    pub origin: Origin,
    pub file: String,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
    /// Lower-case tag name of the clicked element, used to find its tag.
    #[serde(default)]
    pub tag: String,
}

/// A validated location: workspace-relative path (forward slashes),
/// 1-based line, 1-based column (0 = unknown).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLoc {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub tag: String,
}

/// Source files edit mode may open or change.
pub const SOURCE_EXTS: &[&str] = &[
    "svelte", "jsx", "tsx", "js", "ts", "mjs", "vue", "html", "astro",
];

/// Parse a `data-na-src` value: `path:line:col` (the path may contain `:`
/// on Windows, so split from the right).
#[must_use]
pub fn parse_attr(value: &str) -> Option<(String, u32, u32)> {
    let mut parts = value.rsplitn(3, ':');
    let col = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    let file = parts.next()?.to_string();
    (!file.is_empty()).then_some((file, line, col))
}

/// Turn whatever path form the page used into a workspace-relative path:
/// strips URL origins, query strings, `file://`, Vite's `/@fs` prefix and an
/// absolute workspace prefix. Lexical only; [`resolve`] checks the disk.
///
/// # Errors
/// [`DesignError::Outside`] for paths that leave the workspace, are empty or
/// absolute elsewhere.
pub fn relative_path(root: &Path, raw: &str) -> Result<String, DesignError> {
    let mut p = raw.trim();
    // A dev-server URL: keep only its path.
    for scheme in ["http://", "https://"] {
        if let Some(rest) = p.strip_prefix(scheme) {
            p = rest.find('/').map_or("", |i| &rest[i..]);
        }
    }
    p = p.split(['?', '#']).next().unwrap_or_default();
    p = p.strip_prefix("file://").unwrap_or(p);
    let owned;
    if let Some(rest) = p.strip_prefix("/@fs") {
        owned = rest.to_string();
        p = &owned;
    }
    let path = Path::new(p);
    let rel: PathBuf = if path.is_absolute() || looks_windows_absolute(p) {
        let normalized = p.replace('\\', "/");
        let root_str = root.to_string_lossy().replace('\\', "/");
        let root_str = root_str.trim_end_matches('/');
        match normalized.strip_prefix(root_str) {
            Some(rest) if rest.starts_with('/') => PathBuf::from(rest.trim_start_matches('/')),
            _ => {
                // Vite serves workspace files as `/src/...` (root-relative URL path).
                if p.starts_with('/') && !Path::new(p).exists() {
                    PathBuf::from(p.trim_start_matches('/'))
                } else {
                    return Err(DesignError::Outside(raw.to_string()));
                }
            }
        }
    } else {
        PathBuf::from(p)
    };
    let mut clean = Vec::new();
    for part in rel.components() {
        match part {
            Component::Normal(s) => clean.push(s.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => return Err(DesignError::Outside(raw.to_string())),
        }
    }
    if clean.is_empty() {
        return Err(DesignError::Outside(raw.to_string()));
    }
    Ok(clean.join("/"))
}

fn looks_windows_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Validate a raw location against the disk: the file must be an existing
/// regular source file whose real path is inside the workspace's real path
/// (no symlink escapes). Columns are converted to 1-based.
///
/// # Errors
/// [`DesignError::Outside`] or [`DesignError::NotSource`].
pub fn resolve(root: &Path, raw: &RawLocation) -> Result<SourceLoc, DesignError> {
    let rel = relative_path(root, &raw.file)?;
    check_source_file(root, &rel)?;
    let (line, column) = match raw.origin {
        Origin::Attr | Origin::React => (raw.line, raw.column),
        Origin::Svelte => (raw.line, raw.column + 1),
        Origin::Svelte4 => (raw.line + 1, raw.column + 1),
        Origin::Vue => (0, 0),
    };
    let tag = raw
        .tag
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.' || *c == ':')
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    Ok(SourceLoc {
        file: rel,
        line,
        column,
        tag,
    })
}

/// The real path of a workspace-relative source file, refusing anything that
/// is not a regular source file inside the workspace.
///
/// # Errors
/// [`DesignError::Outside`] or [`DesignError::NotSource`].
pub fn check_source_file(root: &Path, rel: &str) -> Result<PathBuf, DesignError> {
    let ext = Path::new(rel)
        .extension()
        .map_or_default(|e| e.to_string_lossy().to_ascii_lowercase());
    if !SOURCE_EXTS.contains(&ext.as_str()) {
        return Err(DesignError::NotSource(rel.to_string()));
    }
    if rel.split('/').any(|part| {
        part == "node_modules" || part == ".git" || part == ".env" || part.starts_with(".env.")
    }) {
        return Err(DesignError::NotSource(rel.to_string()));
    }
    let real_root = root
        .canonicalize()
        .map_err(|e| DesignError::Io(format!("{}: {e}", root.display())))?;
    let resolved = root
        .join(rel)
        .canonicalize()
        .map_err(|_| DesignError::NotSource(rel.to_string()))?;
    if !resolved.starts_with(&real_root) {
        return Err(DesignError::Outside(rel.to_string()));
    }
    let meta = std::fs::metadata(&resolved).map_err(|e| DesignError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(DesignError::NotSource(rel.to_string()));
    }
    Ok(resolved)
}

/// Lines `line - context ..= line + context` of `text` (1-based, clamped),
/// for the "jump to source" panel.
#[must_use]
pub fn snippet(text: &str, line: u32, context: u32) -> (u32, Vec<String>) {
    let total = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
    let line = line.clamp(1, total.max(1));
    let first = line.saturating_sub(context).max(1);
    let last = (line + context).min(total);
    let lines = text
        .lines()
        .skip((first - 1) as usize)
        .take((last + 1).saturating_sub(first) as usize)
        .map(str::to_string)
        .collect();
    (first, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().canonicalize().expect("canon");
        std::fs::create_dir_all(root.join("src/routes")).expect("mk");
        std::fs::write(root.join("src/routes/+page.svelte"), "<h1>Hi</h1>\n").expect("w");
        std::fs::write(root.join("src/App.tsx"), "export const A = () => <div/>;\n").expect("w");
        std::fs::write(root.join("src/notes.md"), "x").expect("w");
        (dir, root)
    }

    #[test]
    fn parses_attr_values() {
        assert_eq!(
            parse_attr("src/A.tsx:12:5"),
            Some(("src/A.tsx".into(), 12, 5))
        );
        assert_eq!(
            parse_attr("C:\\p\\A.tsx:3:1"),
            Some(("C:\\p\\A.tsx".into(), 3, 1))
        );
        assert_eq!(parse_attr("nope"), None);
        assert_eq!(parse_attr(":1:2"), None);
    }

    #[test]
    fn normalises_path_forms() {
        let (_d, root) = ws();
        let abs = format!("{}/src/App.tsx", root.display());
        for raw in [
            "src/App.tsx",
            "./src/App.tsx",
            abs.as_str(),
            &format!("/@fs{abs}"),
            &format!("file://{abs}"),
            "http://localhost:5173/src/App.tsx?t=123",
            "/src/App.tsx",
        ] {
            assert_eq!(
                relative_path(&root, raw).expect(raw),
                "src/App.tsx",
                "{raw}"
            );
        }
    }

    #[test]
    fn refuses_escapes() {
        let (_d, root) = ws();
        for raw in [
            "../secret.tsx",
            "src/../../x.tsx",
            "/etc/passwd",
            "",
            "C:\\Windows\\x.tsx",
        ] {
            assert!(
                relative_path(&root, raw).is_err()
                    || resolve(
                        &root,
                        &RawLocation {
                            origin: Origin::Attr,
                            file: raw.into(),
                            line: 1,
                            column: 1,
                            tag: String::new()
                        }
                    )
                    .is_err(),
                "{raw}"
            );
        }
        assert!(matches!(
            relative_path(&root, "../x.tsx"),
            Err(DesignError::Outside(_))
        ));
    }

    #[test]
    fn resolves_origins_to_one_based() {
        let (_d, root) = ws();
        let svelte = RawLocation {
            origin: Origin::Svelte,
            file: "src/routes/+page.svelte".into(),
            line: 1,
            column: 0,
            tag: "H1".into(),
        };
        let loc = resolve(&root, &svelte).expect("ok");
        assert_eq!((loc.line, loc.column, loc.tag.as_str()), (1, 1, "h1"));
        let s4 = RawLocation {
            origin: Origin::Svelte4,
            line: 0,
            ..svelte.clone()
        };
        assert_eq!(resolve(&root, &s4).expect("ok").line, 1);
        let vue = RawLocation {
            origin: Origin::Vue,
            ..svelte
        };
        assert_eq!(resolve(&root, &vue).expect("ok").line, 0);
    }

    #[test]
    fn refuses_non_source_and_missing_files() {
        let (_d, root) = ws();
        let raw = |file: &str| RawLocation {
            origin: Origin::Attr,
            file: file.into(),
            line: 1,
            column: 1,
            tag: String::new(),
        };
        assert!(matches!(
            resolve(&root, &raw("src/notes.md")),
            Err(DesignError::NotSource(_))
        ));
        assert!(matches!(
            resolve(&root, &raw("src/Missing.tsx")),
            Err(DesignError::NotSource(_))
        ));
        assert!(matches!(
            resolve(&root, &raw("node_modules/x/a.js")),
            Err(DesignError::NotSource(_))
        ));
        assert!(resolve(&root, &raw("src")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_escape() {
        let (_d, root) = ws();
        let outside = tempfile::tempdir().expect("tmp");
        std::fs::write(outside.path().join("x.tsx"), "x").expect("w");
        std::os::unix::fs::symlink(outside.path().join("x.tsx"), root.join("src/link.tsx"))
            .expect("ln");
        let raw = RawLocation {
            origin: Origin::Attr,
            file: "src/link.tsx".into(),
            line: 1,
            column: 1,
            tag: String::new(),
        };
        assert!(matches!(resolve(&root, &raw), Err(DesignError::Outside(_))));
    }

    #[test]
    fn snippets_clamp() {
        let text = "a\nb\nc\nd\ne";
        assert_eq!(
            snippet(text, 3, 1),
            (2, vec!["b".into(), "c".into(), "d".into()])
        );
        assert_eq!(snippet(text, 1, 2).0, 1);
        assert_eq!(snippet(text, 99, 1), (4, vec!["d".into(), "e".into()]));
    }
}
