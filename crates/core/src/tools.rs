//! Tool requests emitted by agents, and their results.

use std::path::PathBuf;

/// A side effect an agent wants to perform. Never executed by the agent itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRequest {
    Read {
        path: PathBuf,
    },
    ListDir {
        path: PathBuf,
    },
    Search {
        pattern: String,
        path: PathBuf,
    },
    Write {
        path: PathBuf,
        contents: String,
    },
    /// Apply a unified diff to one file. Policy treats it as a write.
    ApplyPatch {
        path: PathBuf,
        patch: String,
    },
    /// Find-and-replace in one file with tiered matching
    /// ([`crate::edit_match`]). Policy treats it as a write.
    Edit {
        path: PathBuf,
        old_string: String,
        new_string: String,
        replace_all: bool,
    },
    Exec {
        command: String,
        cwd: PathBuf,
    },
    Network {
        url: String,
    },
    AskUser {
        question: String,
    },
}

impl ToolRequest {
    /// Stable lowercase tool name used in prompts, events and rule patterns.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Read { .. } => "read_file",
            Self::ListDir { .. } => "list_dir",
            Self::Search { .. } => "search",
            Self::Write { .. } | Self::ApplyPatch { .. } | Self::Edit { .. } => "write_file",
            Self::Exec { .. } => "run_command",
            Self::Network { .. } => "http_fetch",
            Self::AskUser { .. } => "ask_user",
        }
    }

    /// Agent-facing tool name (what the model called). Differs from
    /// [`ToolRequest::kind`] only for `apply_patch` and `edit`, which policy,
    /// rules and audit treat as `write_file`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ApplyPatch { .. } => "apply_patch",
            Self::Edit { .. } => "edit",
            other => other.kind(),
        }
    }

    /// Whether the request can change state on disk or the system.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Write { .. } | Self::ApplyPatch { .. } | Self::Edit { .. } | Self::Exec { .. }
        )
    }

    /// Arguments as JSON, for `tool_requested` events. File contents and
    /// patches are reported by size, not inlined.
    #[must_use]
    pub fn args_json(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Self::Read { path } | Self::ListDir { path } => json!({ "path": path }),
            Self::Search { pattern, path } => json!({ "pattern": pattern, "path": path }),
            Self::Write { path, contents } => json!({ "path": path, "bytes": contents.len() }),
            Self::ApplyPatch { path, patch } => json!({ "path": path, "patch": patch }),
            Self::Edit {
                path,
                old_string,
                new_string,
                replace_all,
            } => json!({
                "path": path,
                "old_string": old_string,
                "new_string": new_string,
                "replace_all": replace_all,
            }),
            Self::Exec { command, cwd } => json!({ "command": command, "cwd": cwd }),
            Self::Network { url } => json!({ "url": url }),
            Self::AskUser { question } => json!({ "question": question }),
        }
    }

    /// Path the request targets, if any (for sandbox checks).
    #[must_use]
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Read { path }
            | Self::ListDir { path }
            | Self::Search { path, .. }
            | Self::Write { path, .. }
            | Self::ApplyPatch { path, .. }
            | Self::Edit { path, .. }
            | Self::Exec { cwd: path, .. } => Some(path),
            Self::Network { .. } | Self::AskUser { .. } => None,
        }
    }
}

/// Outcome of an executed tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub call_id: String,
    pub ok: bool,
    pub output: String,
    /// Process exit code for `run_command`.
    pub exit_code: Option<i32>,
}

impl ToolResult {
    #[must_use]
    pub fn ok(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            ok: true,
            output: output.into(),
            exit_code: None,
        }
    }

    #[must_use]
    pub fn error(call_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            ok: false,
            output: message.into(),
            exit_code: None,
        }
    }

    #[must_use]
    pub fn with_exit_code(mut self, code: Option<i32>) -> Self {
        self.exit_code = code;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_event_names() {
        let path = PathBuf::from("/w/a.txt");
        assert_eq!(ToolRequest::Read { path: path.clone() }.kind(), "read_file");
        assert_eq!(
            ToolRequest::ListDir { path: path.clone() }.kind(),
            "list_dir"
        );
        assert_eq!(
            ToolRequest::Search {
                pattern: "x".into(),
                path: path.clone()
            }
            .kind(),
            "search"
        );
        assert_eq!(
            ToolRequest::Write {
                path: path.clone(),
                contents: String::new()
            }
            .kind(),
            "write_file"
        );
        assert_eq!(
            ToolRequest::Exec {
                command: "ls".into(),
                cwd: path.clone()
            }
            .kind(),
            "run_command"
        );
        assert_eq!(
            ToolRequest::Network {
                url: "http://x".into()
            }
            .kind(),
            "http_fetch"
        );
        assert_eq!(
            ToolRequest::AskUser {
                question: "ok?".into()
            }
            .kind(),
            "ask_user"
        );
    }

    #[test]
    fn apply_patch_is_a_write_for_policy() {
        let patch = ToolRequest::ApplyPatch {
            path: PathBuf::from("/w/a"),
            patch: "@@ -1 +1 @@\n-a\n+b\n".into(),
        };
        assert_eq!(patch.kind(), "write_file");
        assert_eq!(patch.name(), "apply_patch");
        assert!(patch.is_mutating());
        assert_eq!(patch.path(), Some(&PathBuf::from("/w/a")));
        assert_eq!(patch.args_json()["path"], "/w/a");
        let write = ToolRequest::Write {
            path: PathBuf::from("/w/a"),
            contents: "abc".into(),
        };
        assert_eq!(write.name(), "write_file");
        assert_eq!(write.args_json()["bytes"], 3);
    }

    #[test]
    fn edit_is_a_write_for_policy() {
        let edit = ToolRequest::Edit {
            path: PathBuf::from("/w/a"),
            old_string: "a".into(),
            new_string: "b".into(),
            replace_all: false,
        };
        assert_eq!(edit.kind(), "write_file");
        assert_eq!(edit.name(), "edit");
        assert!(edit.is_mutating());
        assert_eq!(edit.args_json()["old_string"], "a");
    }

    #[test]
    fn only_writes_and_execs_mutate() {
        assert!(ToolRequest::Write {
            path: PathBuf::from("/w/a"),
            contents: String::new()
        }
        .is_mutating());
        assert!(ToolRequest::Exec {
            command: "ls".into(),
            cwd: PathBuf::from("/w")
        }
        .is_mutating());
        assert!(!ToolRequest::Read {
            path: PathBuf::from("/w/a")
        }
        .is_mutating());
    }

    #[test]
    fn paths_are_exposed_for_sandbox_checks() {
        assert_eq!(
            ToolRequest::Read {
                path: PathBuf::from("/w/a")
            }
            .path(),
            Some(&PathBuf::from("/w/a"))
        );
        assert_eq!(
            ToolRequest::Network {
                url: "http://x".into()
            }
            .path(),
            None
        );
    }
}
