//! Wire shapes for the diff reviewer (matches `src/lib/diff.ts`).

use next_agent_core::worktree::{FileDiff, FileStatus, LineKind};
use next_agent_core::{MergeOutcome, StructuredDiff};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiffLineDto {
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunkDto {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineDto>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffFileDto {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: &'static str,
    pub binary: bool,
    pub hunks: Vec<DiffHunkDto>,
}

/// Result of a Merge / Discard / Keep action.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReviewOutcome {
    pub action: String,
    pub commit: Option<String>,
    pub files: Vec<String>,
}

impl ReviewOutcome {
    #[must_use]
    pub fn merged(outcome: &MergeOutcome) -> Self {
        Self {
            action: "merge".into(),
            commit: outcome.commit.clone(),
            files: outcome.files_changed.clone(),
        }
    }

    #[must_use]
    pub fn simple(action: &str) -> Self {
        Self {
            action: action.into(),
            commit: None,
            files: Vec::new(),
        }
    }
}

const fn status_label(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed => "renamed",
    }
}

const fn line_label(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Context => "context",
        LineKind::Added => "add",
        LineKind::Removed => "remove",
    }
}

fn file_dto(file: &FileDiff) -> DiffFileDto {
    DiffFileDto {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        status: status_label(file.status),
        binary: file.binary,
        hunks: file
            .hunks
            .iter()
            .map(|h| DiffHunkDto {
                old_start: h.old_start,
                old_lines: h.old_lines,
                new_start: h.new_start,
                new_lines: h.new_lines,
                lines: h
                    .lines
                    .iter()
                    .map(|l| DiffLineDto {
                        kind: line_label(l.kind),
                        text: l.text.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Convert the core diff into the UI shape.
#[must_use]
pub fn diff_dto(diff: &StructuredDiff) -> Vec<DiffFileDto> {
    diff.files.iter().map(file_dto).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_agent_core::worktree::parse_unified_diff;

    #[test]
    fn converts_hunks_to_ui_labels() {
        let raw = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";
        let files = diff_dto(&parse_unified_diff(raw));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "modified");
        let kinds: Vec<&str> = files[0].hunks[0].lines.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, ["context", "remove", "add"]);
        let json = serde_json::to_value(&files[0]).expect("json");
        assert_eq!(json["hunks"][0]["oldStart"], 1);
    }
}
