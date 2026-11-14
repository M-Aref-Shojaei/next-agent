//! Vault task documentation (M1 step 5).
//!
//! Turns a finished task's event list into a Markdown record and links it
//! from the day's journal, in a Logseq-compatible graph layout:
//!
//! - record: a page named `Next Agent/<workspace>/Tasks/YYYY-MM-DD-<slug>`,
//!   stored as `<vault>/pages/Next Agent___<workspace>___Tasks___….md`
//!   (triple-lowbar file name), YAML frontmatter + Logseq blocks;
//! - journal: a `- [[<page>]] — done (auto)` block in
//!   `<vault>/journals/yyyy_MM_dd.md` (or the vault's
//!   `:journal/file-name-format`).
//!
//! A vault without `logseq/config.edn` gets a minimal one. See
//! `docs/architecture/Vault Documentation Service.md` and
//! `docs/templates/Task Record Template.md`.
//!
//! Vault writes are internal (not agent tool calls) but are audited through
//! [`Store::append_audit`]. Path and slug rules must stay in lock-step with
//! `packages/vault/src/index.ts`; `fixtures/vault/path-cases.tsv` and
//! `fixtures/vault/page-file-names.tsv` are checked by both sides.

#![allow(
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::map_unwrap_or
)]

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use next_agent_graph::{page_file_name, Block, Date, Document, Graph, GraphError};

use crate::events::{AgentEvent, EventKind};
use crate::store::{AuditEntry, Store, StoreError};
use crate::task::{TaskId, TaskStatus};
use crate::tools::ToolRequest;

/// Everything after this line belongs to the user and survives re-renders.
pub const END_MARKER: &str = "<!-- next-agent:end -->";
const NOTES_HINT: &str = "User notes below this line are preserved.";
const MAX_SLUG: usize = 80;

// ---------------------------------------------------------------- paths --

/// Lowercase, hyphenated file slug. Mirrors `slugify` in `packages/vault`.
///
/// Latin-1 accented letters are folded to ASCII (what NFKD + stripping
/// combining marks does in the TS version); any other non-ASCII character
/// becomes a separator in both implementations.
#[must_use]
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in title.chars() {
        let folded = fold_latin1(ch).unwrap_or(ch).to_ascii_lowercase();
        if folded.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(folded);
        } else {
            pending_dash = true;
        }
    }
    out.truncate(MAX_SLUG);
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

fn fold_latin1(ch: char) -> Option<char> {
    Some(match ch {
        'À'..='Å' => 'A',
        'à'..='å' => 'a',
        'Ç' => 'C',
        'ç' => 'c',
        'È'..='Ë' => 'E',
        'è'..='ë' => 'e',
        'Ì'..='Ï' => 'I',
        'ì'..='ï' => 'i',
        'Ñ' => 'N',
        'ñ' => 'n',
        'Ò'..='Ö' => 'O',
        'ò'..='ö' => 'o',
        'Ù'..='Ü' => 'U',
        'ù'..='ü' => 'u',
        'Ý' => 'Y',
        'ý' | 'ÿ' => 'y',
        _ => return None,
    })
}

/// `2026-11-10-slug`: the record's date and slug.
///
/// The date is the calendar date written in the timestamp (the user's local
/// date, since timestamps carry their offset), never re-interpreted.
#[must_use]
pub fn task_record_stem(iso: &str, title: &str) -> Option<String> {
    let d = Date::from_iso(iso)?;
    Some(format!(
        "{:04}-{:02}-{:02}-{}",
        d.year,
        d.month,
        d.day,
        slugify(title)
    ))
}

/// Logseq page name of a record: `Next Agent/<workspace>/Tasks/<stem>`.
#[must_use]
pub fn task_page_title(workspace: &str, stem: &str) -> String {
    format!("Next Agent/{}/Tasks/{stem}", workspace_folder(workspace))
}

/// `pages/Next Agent___<workspace>___Tasks___2026-11-10-slug.md` relative to
/// the vault root.
#[must_use]
pub fn task_record_path(iso: &str, workspace: &str, title: &str) -> Option<String> {
    let stem = task_record_stem(iso, title)?;
    Some(format!(
        "pages/{}",
        page_file_name(&task_page_title(workspace, &stem))
    ))
}

/// `journals/2026_11_10.md` (Logseq's default `:journal/file-name-format`).
/// The writer uses the vault's own format when its config sets one.
#[must_use]
pub fn journal_path(iso: &str) -> Option<String> {
    let d = Date::from_iso(iso)?;
    Some(format!(
        "journals/{:04}_{:02}_{:02}.md",
        d.year, d.month, d.day
    ))
}

/// Journal block for a record page, e.g.
/// `- [[Next Agent/my-app/Tasks/2026-11-10-x]] — done (auto)`.
#[must_use]
pub fn journal_line(page: &str, status: TaskStatus, mode: &str) -> String {
    let word = match status {
        TaskStatus::Completed => "done",
        other => other.as_str(),
    };
    format!("- [[{page}]] — {word} ({mode})")
}

/// Folder name for a workspace: path separators and control characters are
/// replaced so a workspace name can never escape its folder.
#[must_use]
pub fn workspace_folder(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == ' ' || c == '-');
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}

// ----------------------------------------------------------------- input --

/// Task metadata that is not in the event list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMeta {
    pub id: TaskId,
    /// Defaults to the first line of `prompt`.
    pub title: Option<String>,
    pub prompt: String,
    pub workspace: String,
    /// Used to show file paths relative to the workspace.
    pub workspace_root: Option<PathBuf>,
    pub mode: String,
    pub branch: String,
    pub base: String,
    pub merged: bool,
    pub model: String,
    /// ISO-8601 with offset, e.g. `2026-11-10T10:02:11+03:30`.
    pub started: String,
    pub finished: String,
}

impl TaskMeta {
    fn display_title(&self) -> String {
        let raw = self
            .title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| self.prompt.lines().next().unwrap_or(""));
        let flat = one_line(raw);
        let title: String = flat.chars().take(100).collect();
        if title.is_empty() {
            "Untitled task".to_string()
        } else {
            title
        }
    }
}

/// Diff statistics for one file (from the worktree session, when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// Input to the writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDoc {
    pub meta: TaskMeta,
    pub events: Vec<AgentEvent>,
    /// Request details by call id. Events only carry tool names, so callers
    /// that hold the [`ToolRequest`]s pass them here to name files/commands.
    pub calls: Vec<(String, ToolRequest)>,
    /// Authoritative diff stats; when empty they are derived from writes.
    pub diff_stats: Vec<FileStat>,
}

// -------------------------------------------------------------- analysis --

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRun {
    pub command: String,
    /// `ok`, `failed` or `denied`.
    pub outcome: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionLine {
    pub text: String,
}

/// Facts derived deterministically from the event list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFacts {
    pub status: TaskStatus,
    pub files: Vec<FileStat>,
    pub commands: Vec<CommandRun>,
    pub decisions: Vec<DecisionLine>,
    pub tool_calls: usize,
    pub allowed: usize,
    pub denied: usize,
    pub asked: usize,
    /// `summary` of `task_completed`, or `error` of `task_failed`.
    pub agent_note: Option<String>,
}

fn describe(request: Option<&ToolRequest>, tool: &str, root: Option<&Path>) -> String {
    let rel = |p: &Path| -> String {
        root.and_then(|r| p.strip_prefix(r).ok())
            .unwrap_or(p)
            .display()
            .to_string()
    };
    match request {
        Some(ToolRequest::Exec { command, .. }) => command.clone(),
        Some(
            ToolRequest::Write { path, .. }
            | ToolRequest::ApplyPatch { path, .. }
            | ToolRequest::Read { path }
            | ToolRequest::ListDir { path },
        ) => {
            format!("{tool} {}", rel(path))
        }
        Some(ToolRequest::Search { pattern, .. }) => format!("{tool} {pattern}"),
        Some(ToolRequest::Network { url }) => url.clone(),
        Some(ToolRequest::AskUser { question }) => format!("{tool} {question}"),
        None => tool.to_string(),
    }
}

/// Derive outcome, files, commands and decisions from the events.
///
/// # Errors
/// [`VaultError::NotFinished`] when no terminal event is present.
pub fn analyze(doc: &TaskDoc) -> Result<TaskFacts, VaultError> {
    let root = doc.meta.workspace_root.as_deref();
    let request_for = |id: &str| doc.calls.iter().find(|(c, _)| c == id).map(|(_, r)| r);

    let mut status = None;
    let mut agent_note = None;
    let mut tool_calls = 0;
    let (mut allowed, mut denied, mut asked) = (0, 0, 0);
    let mut files: Vec<FileStat> = Vec::new();
    let mut commands: Vec<CommandRun> = Vec::new();
    let mut decisions: Vec<DecisionLine> = Vec::new();

    // Per-call state in order of first appearance.
    struct Call {
        id: String,
        tool: String,
        decision: Option<String>,
        approval: Option<String>,
        ok: Option<bool>,
    }
    let mut calls: Vec<Call> = Vec::new();

    for event in &doc.events {
        match &event.kind {
            EventKind::ToolRequested { call_id, tool, .. } => {
                tool_calls += 1;
                calls.push(Call {
                    id: call_id.clone(),
                    tool: tool.clone(),
                    decision: None,
                    approval: None,
                    ok: None,
                });
            }
            EventKind::PermissionDecided {
                call_id, decision, ..
            } => {
                if let Some(c) = calls.iter_mut().find(|c| &c.id == call_id) {
                    c.decision = Some(decision.clone());
                }
            }
            EventKind::ApprovalRequested { call_id, .. } => {
                if let Some(c) = calls.iter_mut().find(|c| &c.id == call_id) {
                    c.approval.get_or_insert_with(String::new);
                }
            }
            EventKind::ApprovalResolved {
                approval_id,
                choice,
                ..
            } => {
                // Approval ids are resolved against the request that named them.
                let call_id = doc.events.iter().find_map(|e| match &e.kind {
                    EventKind::ApprovalRequested {
                        approval_id: a,
                        call_id,
                        ..
                    } if a == approval_id => Some(call_id.clone()),
                    _ => None,
                });
                if let Some(c) = call_id.and_then(|id| calls.iter_mut().find(|c| c.id == id)) {
                    c.approval = Some(choice.clone());
                }
            }
            EventKind::ToolFinished { call_id, ok, .. } => {
                if let Some(c) = calls.iter_mut().find(|c| &c.id == call_id) {
                    c.ok = Some(*ok);
                }
            }
            EventKind::TaskCompleted { summary } => {
                status = Some(TaskStatus::Completed);
                agent_note = Some(summary.clone());
            }
            EventKind::TaskFailed { error } => {
                status = Some(TaskStatus::Failed);
                agent_note = Some(error.clone());
            }
            EventKind::TaskCancelled => status = Some(TaskStatus::Cancelled),
            _ => {}
        }
    }

    for call in &calls {
        let request = request_for(&call.id);
        let what = describe(request, &call.tool, root);
        let user_denied = call
            .approval
            .as_deref()
            .is_some_and(|c| c.to_ascii_lowercase().contains("deny"));
        let was_asked = call.approval.is_some() || call.decision.as_deref() == Some("ask");
        let decision = call.decision.as_deref().unwrap_or("allow");
        let blocked = decision == "deny" || user_denied;
        if was_asked {
            asked += 1;
        }
        if blocked {
            denied += 1;
        } else if decision == "allow" || (was_asked && call.approval.is_some()) {
            allowed += 1;
        }

        let line = match (decision, call.approval.as_deref()) {
            _ if user_denied => format!("Asked → Denied by user: `{what}`"),
            ("deny", _) => format!("Denied (policy): `{what}`"),
            ("ask", None) => format!("Asked → no approval available, blocked: `{what}`"),
            (_, Some(_)) => format!("Asked → Allowed by user: `{what}`"),
            _ => format!("Allowed ({}): `{what}`", doc.meta.mode),
        };
        decisions.push(DecisionLine { text: line });

        let executed = !blocked && call.ok == Some(true);
        match request {
            Some(ToolRequest::Exec { command, .. }) => commands.push(CommandRun {
                command: command.clone(),
                outcome: if blocked {
                    "denied"
                } else if call.ok == Some(true) {
                    "ok"
                } else {
                    "failed"
                },
            }),
            None if call.tool == "run_command" => commands.push(CommandRun {
                command: "(command not recorded)".to_string(),
                outcome: if blocked {
                    "denied"
                } else if call.ok == Some(true) {
                    "ok"
                } else {
                    "failed"
                },
            }),
            Some(ToolRequest::Write { path, contents }) if executed => {
                let rel = root
                    .and_then(|r| path.strip_prefix(r).ok())
                    .unwrap_or(path)
                    .display()
                    .to_string();
                let added = u32::try_from(contents.lines().count()).unwrap_or(u32::MAX);
                match files.iter_mut().find(|f| f.path == rel) {
                    Some(existing) => existing.added = existing.added.saturating_add(added),
                    None => files.push(FileStat {
                        path: rel,
                        added,
                        removed: 0,
                    }),
                }
            }
            _ => {}
        }
    }

    if !doc.diff_stats.is_empty() {
        files.clone_from(&doc.diff_stats);
    }

    Ok(TaskFacts {
        status: status.ok_or(VaultError::NotFinished)?,
        files,
        commands,
        decisions,
        tool_calls,
        allowed,
        denied,
        asked,
        agent_note,
    })
}

// ------------------------------------------------------------ summarizer --

/// Short narrative sections of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub summary: String,
    pub follow_ups: Vec<String>,
}

/// Pluggable summary source. The router session provides an LLM-backed
/// implementation; any `Err` falls back to [`DeterministicSummarizer`].
pub trait Summarizer {
    /// # Errors
    /// A human-readable reason when no summary could be produced.
    fn summarize(&self, doc: &TaskDoc, facts: &TaskFacts) -> Result<Summary, String>;
}

/// Always-available summary built only from [`TaskFacts`].
#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicSummarizer;

impl Summarizer for DeterministicSummarizer {
    fn summarize(&self, doc: &TaskDoc, facts: &TaskFacts) -> Result<Summary, String> {
        let outcome = match facts.status {
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "was cancelled",
            TaskStatus::Pending | TaskStatus::Running => "did not finish",
        };
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let mut text = format!(
            "The task {outcome} in {} mode with {} ({} allowed, {} denied, {} asked). \
             It changed {} and ran {}.",
            doc.meta.mode,
            plural(facts.tool_calls, "tool call"),
            facts.allowed,
            facts.denied,
            facts.asked,
            plural(facts.files.len(), "file"),
            plural(facts.commands.len(), "command"),
        );
        if let Some(note) = facts.agent_note.as_deref().filter(|n| !n.trim().is_empty()) {
            let label = if facts.status == TaskStatus::Failed {
                "Error"
            } else {
                "Agent note"
            };
            text.push_str(&format!(" {label}: {}", one_line(note)));
        }
        let follow_ups = match facts.status {
            TaskStatus::Failed => vec!["Investigate the failure and retry".to_string()],
            _ if !doc.meta.merged => vec!["Review the branch and merge or discard it".to_string()],
            _ => Vec::new(),
        };
        Ok(Summary {
            summary: text,
            follow_ups,
        })
    }
}

// ------------------------------------------------------------- rendering --

fn yaml_scalar(value: &str) -> String {
    let single: String = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    let needs_quotes = single.is_empty()
        || single.contains(": ")
        || single.contains(" #")
        || single.starts_with(|c: char| "[{&*!|>%@`#-?\"'".contains(c));
    if needs_quotes {
        format!("\"{single}\"")
    } else {
        single
    }
}

/// One line of free text, safe inside a block's first line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render the full record for the Logseq page `page`. Pure: same input,
/// same bytes. Frontmatter carries the metadata (Logseq reads it as page
/// properties; `title` is the page name); the body is Logseq blocks.
#[must_use]
pub fn render_record(doc: &TaskDoc, facts: &TaskFacts, summary: &Summary, page: &str) -> String {
    let m = &doc.meta;
    let title = m.display_title();
    let mut out = String::new();
    out.push_str("---\ntype: task\n");
    out.push_str(&format!("id: {}\n", m.id));
    out.push_str(&format!("title: {}\n", yaml_scalar(page)));
    out.push_str(&format!("task: {}\n", yaml_scalar(&title)));
    out.push_str(&format!("workspace: {}\n", yaml_scalar(&m.workspace)));
    out.push_str(&format!("mode: {}\n", yaml_scalar(&m.mode)));
    out.push_str(&format!("status: {}\n", facts.status.as_str()));
    out.push_str(&format!("branch: {}\n", yaml_scalar(&m.branch)));
    out.push_str(&format!("base: {}\n", yaml_scalar(&m.base)));
    out.push_str(&format!("merged: {}\n", m.merged));
    out.push_str(&format!("model: {}\n", yaml_scalar(&m.model)));
    out.push_str(&format!("started: {}\n", m.started));
    out.push_str(&format!("finished: {}\n", m.finished));
    out.push_str("tags: [next-agent/task]\n---\n\n");

    let section = |heading: &str, items: Vec<String>| {
        Block::new(format!("## {heading}"))
            .with_children(items.into_iter().map(Block::new).collect())
    };
    let none = || vec!["None.".to_string()];

    let request = if m.prompt.trim().is_empty() {
        "> (no prompt recorded)".to_string()
    } else {
        m.prompt
            .lines()
            .map(|l| format!("> {}", l.trim_end()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let changes = if facts.files.is_empty() {
        vec!["No files changed.".to_string()]
    } else {
        let mut table = String::from("| File | +/− |\n|---|---|");
        for f in &facts.files {
            table.push_str(&format!(
                "\n| {} | +{} −{} |",
                one_line(&f.path).replace('|', "\\|"),
                f.added,
                f.removed
            ));
        }
        vec![table]
    };
    let commands: Vec<String> = facts
        .commands
        .iter()
        .map(|c| {
            format!(
                "`{}` — {}",
                one_line(&c.command).replace('`', "'"),
                c.outcome
            )
        })
        .collect();
    let decisions: Vec<String> = facts.decisions.iter().map(|d| one_line(&d.text)).collect();
    let follow_ups: Vec<String> = summary
        .follow_ups
        .iter()
        .map(|f| one_line(f))
        .filter(|f| !f.is_empty())
        .map(|f| format!("TODO {f}"))
        .collect();
    let or_none = |items: Vec<String>| if items.is_empty() { none() } else { items };

    let blocks = vec![
        Block::new(format!("# {title}")),
        section("Request", vec![request]),
        section("Summary", vec![summary.summary.trim().to_string()]),
        section("Changes", changes),
        section("Commands run", or_none(commands)),
        section("Permission decisions", or_none(decisions)),
        section("Follow-ups", or_none(follow_ups)),
        Block::new(format!("{END_MARKER}\n{NOTES_HINT}")),
    ];
    out.push_str(&Document::new(blocks).render());
    out
}

/// Keep what the user wrote below the end marker of `existing`.
#[must_use]
pub fn preserve_user_notes(rendered: &str, existing: &str) -> String {
    let (Some(new_at), Some(old_at)) = (rendered.find(END_MARKER), existing.find(END_MARKER))
    else {
        return rendered.to_string();
    };
    let mut out = rendered[..new_at + END_MARKER.len()].to_string();
    out.push_str(&existing[old_at + END_MARKER.len()..]);
    out
}

// ---------------------------------------------------------------- writer --

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    pub record_path: PathBuf,
    pub record: Change,
    pub journal_path: PathBuf,
    pub journal: Change,
    /// True when the summarizer failed and the deterministic text was used.
    pub used_fallback_summary: bool,
}

#[derive(Debug)]
pub enum VaultError {
    NotFinished,
    BadTimestamp(String),
    Io(std::io::Error),
    Audit(StoreError),
}

impl From<GraphError> for VaultError {
    fn from(e: GraphError) -> Self {
        match e {
            GraphError::Io(io) => Self::Io(io),
            other => Self::Io(std::io::Error::other(other.to_string())),
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinished => f.write_str("task has no terminal event yet"),
            Self::BadTimestamp(s) => write!(f, "timestamp must start with YYYY-MM-DD: {s:?}"),
            Self::Io(e) => write!(f, "vault write failed: {e}"),
            Self::Audit(e) => write!(f, "vault audit failed: {e}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Write `contents` to `path` via a temp file in the same directory + rename,
/// so readers see either the old file or the new one, never a partial write.
/// (Shared with the notes engine.)
pub use next_agent_graph::write_atomic;

/// The task id in a record's frontmatter (any line endings, quoted or not).
fn id_of(source: &str) -> Option<String> {
    next_agent_graph::frontmatter::parse(source)
        .into_iter()
        .find(|p| p.key == "id")
        .map(|p| p.value)
}

/// Writes task records into a vault.
#[derive(Debug, Clone)]
pub struct VaultWriter {
    root: PathBuf,
}

impl VaultWriter {
    #[must_use]
    pub fn new(vault_root: impl Into<PathBuf>) -> Self {
        Self {
            root: vault_root.into(),
        }
    }

    /// Document a finished task. Safe to call again for the same task: the
    /// record is re-rendered in place (user notes kept) and the journal link
    /// is never duplicated. Every write that changes a file is audited.
    ///
    /// # Errors
    /// See [`VaultError`].
    pub fn document_task(
        &self,
        store: &Store,
        doc: &TaskDoc,
        summarizer: Option<&dyn Summarizer>,
    ) -> Result<WriteOutcome, VaultError> {
        let facts = analyze(doc)?;
        let (summary, used_fallback_summary) = match summarizer.map(|s| s.summarize(doc, &facts)) {
            Some(Ok(s)) => (s, false),
            Some(Err(_)) => (fallback(doc, &facts), true),
            None => (fallback(doc, &facts), false),
        };
        let bad_ts = || VaultError::BadTimestamp(doc.meta.finished.clone());
        let date = Date::from_iso(&doc.meta.finished).ok_or_else(bad_ts)?;
        let stem =
            task_record_stem(&doc.meta.finished, &doc.meta.display_title()).ok_or_else(bad_ts)?;

        // A vault is a Logseq graph: make sure its folders and config exist.
        let (graph, created_config) = Graph::init(&self.root)?;
        if created_config {
            self.audit(
                store,
                doc,
                &self.root.join(next_agent_graph::layout::CONFIG_PATH),
                "created logseq config",
            )?;
        }

        // Same task id keeps its page; a different task with the same
        // date+slug gets `-2`, `-3`, ... instead of overwriting.
        let id = doc.meta.id.as_str();
        let mut page = task_page_title(&doc.meta.workspace, &stem);
        let mut record_path = graph.resolve(&graph.page_rel_path(&page))?;
        for n in 2.. {
            match fs::read_to_string(&record_path) {
                Ok(existing) if id_of(&existing).as_deref() != Some(id) => {
                    page = task_page_title(&doc.meta.workspace, &format!("{stem}-{n}"));
                    record_path = graph.resolve(&graph.page_rel_path(&page))?;
                }
                _ => break,
            }
        }

        let rendered = render_record(doc, &facts, &summary, &page);
        let existing = fs::read_to_string(&record_path).ok();
        let final_text = existing.as_deref().map_or_else(
            || rendered.clone(),
            |old| preserve_user_notes(&rendered, old),
        );
        let record = match existing {
            None => Change::Created,
            Some(old) if old == final_text => Change::Unchanged,
            Some(_) => Change::Updated,
        };
        if record != Change::Unchanged {
            write_atomic(&record_path, &final_text)?;
            self.audit(
                store,
                doc,
                &record_path,
                if record == Change::Created {
                    "created task record"
                } else {
                    "updated task record"
                },
            )?;
        }

        let journal_path = graph.resolve(&graph.journal_rel_path(date))?;
        let journal = self.update_journal(store, doc, &journal_path, &page, facts.status)?;

        Ok(WriteOutcome {
            record_path,
            record,
            journal_path,
            journal,
            used_fallback_summary,
        })
    }

    fn update_journal(
        &self,
        store: &Store,
        doc: &TaskDoc,
        path: &Path,
        page: &str,
        status: TaskStatus,
    ) -> Result<Change, VaultError> {
        let line = journal_line(page, status, &doc.meta.mode);
        let needle = format!("[[{page}]]");
        let existing = fs::read_to_string(path).ok();
        let (text, change) = match &existing {
            None => (format!("{line}\n"), Change::Created),
            Some(old) if old.lines().any(|l| l == line) => return Ok(Change::Unchanged),
            Some(old) if old.lines().any(|l| l.contains(&needle)) => {
                let mut replaced = false;
                let lines: Vec<String> = old
                    .lines()
                    .filter_map(|l| {
                        if l.contains(&needle) {
                            if replaced {
                                return None; // collapse accidental duplicates
                            }
                            replaced = true;
                            Some(line.clone())
                        } else {
                            Some(l.to_string())
                        }
                    })
                    .collect();
                (lines.join("\n") + "\n", Change::Updated)
            }
            Some(old) => {
                let mut text = old.clone();
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&line);
                text.push('\n');
                (text, Change::Updated)
            }
        };
        write_atomic(path, &text)?;
        self.audit(store, doc, path, "linked task in journal")?;
        Ok(change)
    }

    fn audit(
        &self,
        store: &Store,
        doc: &TaskDoc,
        path: &Path,
        reason: &str,
    ) -> Result<(), VaultError> {
        let target = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string();
        store
            .append_audit(
                &AuditEntry::new(&doc.meta.mode, "vault_write", &target, "allow")
                    .reason(reason)
                    .task(doc.meta.id.as_str()),
            )
            .map(|_| ())
            .map_err(VaultError::Audit)
    }
}

fn fallback(doc: &TaskDoc, facts: &TaskFacts) -> Summary {
    DeterministicSummarizer
        .summarize(doc, facts)
        .unwrap_or(Summary {
            summary: String::new(),
            follow_ups: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventLog;

    const PATH_CASES: &str = include_str!("../../../fixtures/vault/path-cases.tsv");
    const GOLDEN: &str = include_str!("../../../fixtures/vault/golden-task-record.md");
    const GOLDEN_JOURNAL: &str = include_str!("../../../fixtures/vault/golden-journal.md");

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("na-vault-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn sample(finished: &str) -> TaskDoc {
        let id = TaskId::from_seed("golden");
        let mut log = EventLog::new(id.clone());
        let mut events = Vec::new();
        let mut push = |k| events.push(log.push(k));
        push(EventKind::TaskStarted {
            mode: "auto",
            executor: "mock".into(),
            model: String::new(),
        });
        for (call, tool) in [
            ("c1", "write_file"),
            ("c2", "run_command"),
            ("c3", "run_command"),
            ("c4", "run_command"),
        ] {
            push(EventKind::ToolRequested {
                call_id: call.into(),
                tool: tool.into(),
                args: serde_json::Value::Null,
            });
            let decision = if call == "c4" { "deny" } else { "allow" };
            push(EventKind::PermissionDecided {
                call_id: call.into(),
                decision: decision.into(),
                reason: "r".into(),
            });
            push(EventKind::ToolFinished {
                call_id: call.into(),
                ok: call != "c3" && call != "c4",
                exit_code: None,
                summary: String::new(),
            });
        }
        push(EventKind::TaskCompleted {
            summary: "Added validation.".into(),
        });
        TaskDoc {
            meta: TaskMeta {
                id,
                title: Some("Add login form validation".into()),
                prompt: "Add validation to the login form\nand test it".into(),
                workspace: "my-app".into(),
                workspace_root: Some(PathBuf::from("/w/my-app")),
                mode: "auto".into(),
                branch: "next-agent/auto-3f2a".into(),
                base: "a1b2c3d".into(),
                merged: false,
                model: "test-model".into(),
                started: "2026-11-10T10:02:11+03:30".into(),
                finished: finished.into(),
            },
            events,
            calls: vec![
                (
                    "c1".into(),
                    ToolRequest::Write {
                        path: "/w/my-app/src/login.ts".into(),
                        contents: "a\nb\nc\n".into(),
                    },
                ),
                (
                    "c2".into(),
                    ToolRequest::Exec {
                        command: "pnpm test".into(),
                        cwd: "/w/my-app".into(),
                    },
                ),
                (
                    "c3".into(),
                    ToolRequest::Exec {
                        command: "pnpm lint".into(),
                        cwd: "/w/my-app".into(),
                    },
                ),
                (
                    "c4".into(),
                    ToolRequest::Exec {
                        command: "curl https://x.test".into(),
                        cwd: "/w/my-app".into(),
                    },
                ),
            ],
            diff_stats: Vec::new(),
        }
    }

    const FINISHED: &str = "2026-11-10T10:09:40+03:30";
    const PAGE: &str = "Next Agent/my-app/Tasks/2026-11-10-add-login-form-validation";
    const RECORD_FILE: &str =
        "pages/Next Agent___my-app___Tasks___2026-11-10-add-login-form-validation.md";

    #[test]
    fn slug_and_path_cases_match_shared_fixture() {
        let mut checked = 0;
        for line in PATH_CASES
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 5, "bad fixture line {line:?}");
            assert_eq!(
                task_record_path(cols[0], cols[1], cols[2]).as_deref(),
                Some(cols[3]),
                "{line}"
            );
            assert_eq!(journal_path(cols[0]).as_deref(), Some(cols[4]), "{line}");
            checked += 1;
        }
        assert!(checked >= 6);
    }

    #[test]
    fn renders_golden_record() {
        let doc = sample(FINISHED);
        let facts = analyze(&doc).expect("facts");
        let summary = DeterministicSummarizer
            .summarize(&doc, &facts)
            .expect("summary");
        let text = render_record(&doc, &facts, &summary, PAGE);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            fs::write(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../fixtures/vault/golden-task-record.md"
                ),
                &text,
            )
            .expect("write golden");
        }
        assert_eq!(text, GOLDEN);
    }

    #[test]
    fn facts_cover_files_commands_and_decisions() {
        let facts = analyze(&sample(FINISHED)).expect("facts");
        assert_eq!(facts.status, TaskStatus::Completed);
        assert_eq!(
            facts.files,
            vec![FileStat {
                path: "src/login.ts".into(),
                added: 3,
                removed: 0
            }]
        );
        let outcomes: Vec<_> = facts
            .commands
            .iter()
            .map(|c| (c.command.as_str(), c.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [
                ("pnpm test", "ok"),
                ("pnpm lint", "failed"),
                ("curl https://x.test", "denied")
            ]
        );
        assert_eq!((facts.allowed, facts.denied, facts.asked), (3, 1, 0));
    }

    #[test]
    fn unfinished_task_is_rejected() {
        let mut doc = sample(FINISHED);
        doc.events.truncate(3);
        assert!(matches!(analyze(&doc), Err(VaultError::NotFinished)));
    }

    #[test]
    fn writes_record_and_journal_and_audits() {
        let vault = temp("write");
        let store = Store::open_in_memory().expect("store");
        let out = VaultWriter::new(&vault)
            .document_task(&store, &sample(FINISHED), None)
            .expect("write");
        assert_eq!(out.record, Change::Created);
        assert_eq!(out.journal, Change::Created);
        assert_eq!(out.record_path, vault.join(RECORD_FILE));
        assert_eq!(fs::read_to_string(&out.record_path).expect("read"), GOLDEN);
        assert_eq!(out.journal_path, vault.join("journals/2026_11_10.md"));
        let journal = fs::read_to_string(&out.journal_path).expect("journal");
        assert_eq!(journal, GOLDEN_JOURNAL);
        assert!(vault.join("logseq/config.edn").is_file());
        assert!(vault.join("assets").is_dir());
        let report = store.verify_audit().expect("verify");
        assert!(report.ok);
        assert_eq!(report.checked, 3, "config + record + journal");
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn rerun_is_idempotent() {
        let vault = temp("idem");
        let store = Store::open_in_memory().expect("store");
        let writer = VaultWriter::new(&vault);
        let doc = sample(FINISHED);
        writer.document_task(&store, &doc, None).expect("first");
        let again = writer.document_task(&store, &doc, None).expect("second");
        assert_eq!(
            (again.record, again.journal),
            (Change::Unchanged, Change::Unchanged)
        );
        let journal = fs::read_to_string(&again.journal_path).expect("journal");
        assert_eq!(journal.matches(&format!("[[{PAGE}]]")).count(), 1);
        let files = fs::read_dir(again.record_path.parent().expect("dir"))
            .expect("ls")
            .count();
        assert_eq!(files, 1);
        assert_eq!(
            store.verify_audit().expect("verify").checked,
            3,
            "no audit noise on no-op"
        );
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn rerun_updates_merged_and_keeps_user_notes() {
        let vault = temp("notes");
        let store = Store::open_in_memory().expect("store");
        let writer = VaultWriter::new(&vault);
        let mut doc = sample(FINISHED);
        let first = writer.document_task(&store, &doc, None).expect("first");
        let mut text = fs::read_to_string(&first.record_path).expect("read");
        text.push_str("my own note\n");
        fs::write(&first.record_path, text).expect("edit");
        doc.meta.merged = true;
        let second = writer.document_task(&store, &doc, None).expect("second");
        assert_eq!(second.record, Change::Updated);
        let after = fs::read_to_string(&second.record_path).expect("read");
        assert!(after.contains("merged: true"));
        assert!(after.ends_with("my own note\n"));
        assert_eq!(after.matches(END_MARKER).count(), 1);
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn different_task_same_title_does_not_overwrite() {
        let vault = temp("collide");
        let store = Store::open_in_memory().expect("store");
        let writer = VaultWriter::new(&vault);
        let a = sample(FINISHED);
        let mut b = sample(FINISHED);
        b.meta.id = TaskId::from_seed("other");
        let first = writer.document_task(&store, &a, None).expect("a");
        let second = writer.document_task(&store, &b, None).expect("b");
        assert_ne!(first.record_path, second.record_path);
        assert!(second.record_path.to_string_lossy().ends_with("-2.md"));
        let again = writer.document_task(&store, &b, None).expect("b again");
        assert_eq!(again.record, Change::Unchanged);
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn journal_appends_to_existing_day() {
        let vault = temp("journal");
        fs::create_dir_all(vault.join("journals")).expect("mkdir");
        fs::write(
            vault.join("journals/2026_11_10.md"),
            "- my thoughts\n\t- nested",
        )
        .expect("seed");
        let store = Store::open_in_memory().expect("store");
        VaultWriter::new(&vault)
            .document_task(&store, &sample(FINISHED), None)
            .expect("write");
        let journal = fs::read_to_string(vault.join("journals/2026_11_10.md")).expect("read");
        assert_eq!(
            journal,
            format!("- my thoughts\n\t- nested\n{GOLDEN_JOURNAL}")
        );
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn atomic_write_replaces_without_leftovers() {
        let dir = temp("atomic");
        let target = dir.join("nested/file.md");
        write_atomic(&target, "one").expect("create");
        write_atomic(&target, "two").expect("replace");
        assert_eq!(fs::read_to_string(&target).expect("read"), "two");
        let names: Vec<_> = fs::read_dir(target.parent().expect("dir"))
            .expect("ls")
            .map(|e| e.expect("e").file_name())
            .collect();
        assert_eq!(names.len(), 1, "temp file must be gone: {names:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_atomic_write_leaves_target_untouched() {
        let dir = temp("atomic-fail");
        let target = dir.join("file.md");
        write_atomic(&target, "original").expect("create");
        // Rename onto a directory fails after the temp file was written.
        let blocked = dir.join("blocked");
        fs::create_dir_all(blocked.join("child")).expect("mkdir");
        assert!(write_atomic(&blocked, "x").is_err());
        assert_eq!(fs::read_to_string(&target).expect("read"), "original");
        let leftovers = fs::read_dir(&dir)
            .expect("ls")
            .filter(|e| {
                e.as_ref()
                    .expect("e")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    struct Failing;
    impl Summarizer for Failing {
        fn summarize(&self, _: &TaskDoc, _: &TaskFacts) -> Result<Summary, String> {
            Err("router down".into())
        }
    }
    struct Fixed;
    impl Summarizer for Fixed {
        fn summarize(&self, _: &TaskDoc, _: &TaskFacts) -> Result<Summary, String> {
            Ok(Summary {
                summary: "LLM says hi.".into(),
                follow_ups: vec!["Ship it".into()],
            })
        }
    }

    #[test]
    fn summarizer_plugs_in_and_falls_back() {
        let vault = temp("summ");
        let store = Store::open_in_memory().expect("store");
        let writer = VaultWriter::new(&vault);
        let ok = writer
            .document_task(&store, &sample(FINISHED), Some(&Fixed))
            .expect("llm");
        assert!(!ok.used_fallback_summary);
        let text = fs::read_to_string(&ok.record_path).expect("read");
        assert!(text.contains("LLM says hi.") && text.contains("\t- TODO Ship it"));

        let vault2 = temp("summ2");
        let fell = VaultWriter::new(&vault2)
            .document_task(&store, &sample(FINISHED), Some(&Failing))
            .expect("fallback");
        assert!(fell.used_fallback_summary);
        assert_eq!(fs::read_to_string(&fell.record_path).expect("read"), GOLDEN);
        let _ = fs::remove_dir_all(&vault);
        let _ = fs::remove_dir_all(&vault2);
    }

    #[test]
    fn record_is_a_logseq_page() {
        let doc = Document::parse(GOLDEN);
        assert_eq!(
            doc.render(),
            GOLDEN,
            "record round-trips through the notes engine"
        );
        let dir = temp("logseq-page");
        let (graph, _) = Graph::init(&dir).expect("graph");
        assert_eq!(graph.page_name(RECORD_FILE, &doc), PAGE);
        assert_eq!(graph.page_rel_path(PAGE), RECORD_FILE);
        let props = doc.page_properties();
        let prop = |k: &str| props.iter().find(|p| p.key == k).map(|p| p.value.as_str());
        assert_eq!(prop("merged"), Some("false"));
        assert_eq!(prop("tags"), Some("next-agent/task"));
        assert_eq!(doc.blocks[0].content, "# Add login form validation");
        let follow_ups = &doc.blocks[6];
        assert_eq!(follow_ups.content, "## Follow-ups");
        assert_eq!(
            follow_ups.children[0].info().marker.as_deref(),
            Some("TODO")
        );
        assert!(doc.blocks[7].content.starts_with(END_MARKER));
        let journal = Document::parse(GOLDEN_JOURNAL);
        assert!(journal.blocks[0]
            .info()
            .refs
            .iter()
            .any(|r| matches!(r, next_agent_graph::Ref::Page { name, .. } if name == PAGE)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn honours_the_vault_journal_format_and_keeps_its_config() {
        let vault = temp("journal-format");
        fs::create_dir_all(vault.join("logseq")).expect("mkdir");
        let config =
            "{:journal/file-name-format \"yyyy-MM-dd\"\n :file/name-format :triple-lowbar}\n";
        fs::write(vault.join("logseq/config.edn"), config).expect("config");
        let store = Store::open_in_memory().expect("store");
        let out = VaultWriter::new(&vault)
            .document_task(&store, &sample(FINISHED), None)
            .expect("write");
        assert_eq!(out.journal_path, vault.join("journals/2026-11-10.md"));
        assert_eq!(
            fs::read_to_string(vault.join("logseq/config.edn")).expect("read"),
            config
        );
        assert_eq!(store.verify_audit().expect("verify").checked, 2);
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn workspace_names_cannot_escape() {
        assert_eq!(workspace_folder("../../etc"), "etc");
        assert_eq!(workspace_folder("a/b"), "a-b");
        assert_eq!(workspace_folder("..."), "workspace");
    }

    #[test]
    fn rejects_bad_timestamp() {
        let vault = temp("badts");
        let store = Store::open_in_memory().expect("store");
        let mut doc = sample(FINISHED);
        doc.meta.finished = "yesterday".into();
        assert!(matches!(
            VaultWriter::new(&vault).document_task(&store, &doc, None),
            Err(VaultError::BadTimestamp(_))
        ));
        let _ = fs::remove_dir_all(&vault);
    }
}
