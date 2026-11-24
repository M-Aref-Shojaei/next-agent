//! Read-only discovery for the selected project, and creation of new folders.

use crate::design::design_dir;
use crate::service::{AppService, WorkspaceDto};
use next_agent_core::Workspace;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_TREE_ENTRIES: usize = 2_000;
const MAX_TREE_DEPTH: usize = 8;
const IGNORED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    ".DS_Store",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiscovery {
    agent_instructions: Option<String>,
    design_md: Option<String>,
    tokens_css: Option<String>,
    vault_path: Option<PathBuf>,
    files: Vec<ProjectEntry>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct ProjectEntry {
    name: String,
    children: Option<Vec<Self>>,
}

impl AppService {
    /// Create a new project directory directly under an existing parent.
    ///
    /// # Errors
    /// Returns an error for invalid names, existing destinations or I/O errors.
    pub fn workspace_create(&self, parent: &str, name: &str) -> Result<WorkspaceDto, String> {
        let parent = Path::new(parent);
        if !parent.is_absolute()
            || name.is_empty()
            || name == "."
            || name.ends_with(['.', ' '])
            || name.contains(['/', '\\', ':'])
            || name.chars().any(char::is_control)
        {
            return Err("Choose an absolute parent path and a valid project name.".into());
        }
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let root = parent.join(name);
        let path = root.to_str().ok_or("Project path must be valid UTF-8.")?;
        fs::create_dir(&root).map_err(|e| format!("Could not create the project folder: {e}"))?;
        self.workspace_open(path).inspect_err(|_| {
            // Only remove an empty folder that we created.
            let _ = fs::remove_dir(&root);
        })
    }

    /// Discover project connections without changing vault settings or project files.
    ///
    /// # Errors
    /// Returns an error for an unknown project or an unreadable project root.
    pub fn workspace_discover(&self, id: &str) -> Result<WorkspaceDiscovery, String> {
        let workspace = self.workspace(id)?;
        let reopened = Workspace::open(&workspace.path).map_err(|e| e.to_string())?;
        if reopened.id() != id {
            return Err("The project folder now points elsewhere. Open it again to select the new location.".into());
        }
        discover(reopened.root())
    }
}

fn discover(root: &Path) -> Result<WorkspaceDiscovery, String> {
    let design = design_dir(root);
    let design = real_dir(&design).then_some(design);
    let vault_path = ["", "vault", "docs", "notes"].into_iter().find_map(|name| {
        let candidate = root.join(name);
        is_vault(&candidate, name == "vault").then_some(candidate)
    });
    let mut budget = MAX_TREE_ENTRIES;
    let mut truncated = false;
    let entries = fs::read_dir(root).map_err(|e| e.to_string())?;
    let files = file_tree(entries, 1, &mut budget, &mut truncated);
    Ok(WorkspaceDiscovery {
        agent_instructions: relative_file(root, &root.join("AGENTS.md")),
        design_md: design
            .as_ref()
            .and_then(|dir| relative_file(root, &dir.join("DESIGN.md"))),
        tokens_css: design
            .as_ref()
            .and_then(|dir| relative_file(root, &dir.join("tokens.css"))),
        vault_path,
        files,
        truncated,
    })
}

fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

fn relative_file(root: &Path, file: &Path) -> Option<String> {
    if !fs::symlink_metadata(file).is_ok_and(|meta| meta.file_type().is_file()) {
        return None;
    }
    Some(file.strip_prefix(root).ok()?.to_str()?.replace('\\', "/"))
}

fn is_vault(candidate: &Path, named_vault: bool) -> bool {
    let pages = real_dir(&candidate.join("pages"));
    let journals = real_dir(&candidate.join("journals"));
    real_dir(candidate)
        && (real_dir(&candidate.join("logseq"))
            || (pages && journals)
            || (named_vault && (pages || journals)))
        && ["logseq", "pages", "journals", "assets", "logseq/config.edn"]
            .iter()
            .all(|name| {
                !fs::symlink_metadata(candidate.join(name))
                    .is_ok_and(|meta| meta.file_type().is_symlink())
            })
}

/// Run the platform folder chooser with fixed arguments. The sidebar also accepts a path.
pub(crate) fn pick_folder() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "try\nreturn POSIX path of (choose folder with prompt \"Select a project folder\")\non error number -128\nreturn \"\"\nend try",
        ])
        .output();
    #[cfg(target_os = "windows")]
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-STA",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Windows.Forms; $dialog = [Windows.Forms.FolderBrowserDialog]::new(); if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
        ])
        .output();
    #[cfg(all(unix, not(target_os = "macos")))]
    let output = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Select a project folder",
        ])
        .output();

    let output = output.map_err(|e| e.to_string())?;
    if !output.status.success() {
        #[cfg(all(unix, not(target_os = "macos")))]
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        return Err("The folder chooser could not open.".into());
    }
    let path = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let path = path.trim_end_matches(['\r', '\n']);
    Ok((!path.is_empty()).then(|| path.to_string()))
}

fn file_tree(
    entries: fs::ReadDir,
    depth: usize,
    budget: &mut usize,
    truncated: &mut bool,
) -> Vec<ProjectEntry> {
    let mut visible = Vec::new();
    for entry in entries {
        if *budget == 0 {
            *truncated = true;
            break;
        }
        *budget -= 1;
        let Ok(entry) = entry else {
            *truncated = true;
            continue;
        };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if IGNORED.contains(&name.as_str()) || name == ".env" || name.starts_with(".env.") {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            *truncated = true;
            continue;
        };
        if kind.is_dir() || kind.is_file() {
            visible.push((!kind.is_dir(), name, entry.path()));
        }
    }
    visible.sort_unstable();
    visible
        .into_iter()
        .map(|(file, name, path)| {
            let children = if file {
                None
            } else {
                let nested = (depth < MAX_TREE_DEPTH && *budget > 0).then(|| fs::read_dir(path));
                Some(if let Some(Ok(entries)) = nested {
                    file_tree(entries, depth + 1, budget, truncated)
                } else {
                    *truncated = true;
                    Vec::new()
                })
            };
            ProjectEntry { name, children }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_project_connections_without_changing_the_vault() {
        let temp = tempfile::tempdir().unwrap();
        let svc = AppService::new(temp.path().join("db")).unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(root.join("vault/pages")).unwrap();
        fs::create_dir_all(root.join("design")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        for path in ["AGENTS.md", "design/DESIGN.md", "design/tokens.css", ".env"] {
            fs::write(root.join(path), "").unwrap();
        }
        let project = svc.workspace_open(root.to_str().unwrap()).unwrap();
        let discovery = svc.workspace_discover(&project.id).unwrap();
        assert_eq!(discovery.agent_instructions.as_deref(), Some("AGENTS.md"));
        assert_eq!(discovery.design_md.as_deref(), Some("design/DESIGN.md"));
        assert_eq!(discovery.tokens_css.as_deref(), Some("design/tokens.css"));
        assert_eq!(
            discovery.vault_path,
            Some(root.canonicalize().unwrap().join("vault"))
        );
        assert_eq!(
            discovery
                .files
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["design", "vault", "AGENTS.md"]
        );
        assert!(!discovery.truncated);
        assert_eq!(svc.vault_get().unwrap(), None);
        assert!(!root.join("vault/logseq").exists());
    }

    #[test]
    fn creates_a_new_project_without_overwriting_or_escaping() {
        let temp = tempfile::tempdir().unwrap();
        let svc = AppService::new(temp.path().join("db")).unwrap();
        let parent = temp.path().to_str().unwrap();
        for name in [
            "",
            "..",
            "../outside",
            "nested/project",
            "nested\\project",
            "C:outside",
        ] {
            assert!(svc.workspace_create(parent, name).is_err(), "{name}");
        }
        let project = svc.workspace_create(parent, "New Project").unwrap();
        assert!(svc.workspace_create(parent, "New Project").is_err());
        let discovery = svc.workspace_discover(&project.id).unwrap();
        assert!(discovery.files.is_empty());
        assert_eq!(discovery.vault_path, None);
        fs::create_dir(Path::new(&project.path).join("pages")).unwrap();
        assert_eq!(
            svc.workspace_discover(&project.id).unwrap().vault_path,
            None
        );
        assert_eq!(svc.workspace_list().unwrap(), [project]);
    }

    #[cfg(unix)]
    #[test]
    fn discovery_ignores_links_outside_the_project() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("pages")).unwrap();
        fs::write(outside.path().join("AGENTS.md"), "outside").unwrap();
        symlink(outside.path(), root.path().join("vault")).unwrap();
        symlink(
            outside.path().join("AGENTS.md"),
            root.path().join("AGENTS.md"),
        )
        .unwrap();
        let found = discover(root.path()).unwrap();
        assert!(found.files.is_empty());
        assert_eq!(found.agent_instructions, None);
        assert_eq!(found.vault_path, None);

        let svc = AppService::new(root.path().join("db")).unwrap();
        let project_dir = root.path().join("registered");
        fs::create_dir(&project_dir).unwrap();
        let project = svc.workspace_open(project_dir.to_str().unwrap()).unwrap();
        fs::remove_dir(&project_dir).unwrap();
        symlink(outside.path(), &project_dir).unwrap();
        assert!(svc.workspace_discover(&project.id).is_err());
    }
}
