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
            Self::Write { .. } => "write_file",
            Self::Exec { .. } => "run_command",
            Self::Network { .. } => "http_fetch",
            Self::AskUser { .. } => "ask_user",
        }
    }

    /// Whether the request can change state on disk or the system.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        matches!(self, Self::Write { .. } | Self::Exec { .. })
    }

    /// Path the request targets, if any (for sandbox checks).
    #[must_use]
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Read { path } | Self::ListDir { path } | Self::Search { path, .. } => Some(path),
            Self::Write { path, .. } => Some(path),
            Self::Exec { cwd, .. } => Some(cwd),
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
}

impl ToolResult {
    #[must_use]
    pub fn ok(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            ok: true,
            output: output.into(),
        }
    }

    #[must_use]
    pub fn error(call_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            ok: false,
            output: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_event_names() {
        let path = PathBuf::from("/w/a.txt");
        assert_eq!(ToolRequest::Read { path: path.clone() }.kind(), "read_file");
        assert_eq!(ToolRequest::ListDir { path: path.clone() }.kind(), "list_dir");
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
            ToolRequest::Network { url: "http://x".into() }.kind(),
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
            ToolRequest::Network { url: "http://x".into() }.path(),
            None
        );
    }
}
