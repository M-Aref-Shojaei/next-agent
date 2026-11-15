//! AI Design Studio: the app side of `next-agent-design`.
//!
//! - Design system: `DESIGN.md` + `tokens.css` per workspace (root or
//!   `design/`), generated from the project's CSS, editable in the studio.
//! - Generate / refine / convert on the gateway's `design` model role, or the
//!   offline mock model when no provider is configured.
//! - Edit mode: the workspace's dev server (or a loopback URL) behind the
//!   preview proxy, element → source mapping, and style edits written back
//!   to code by a **scripted task in a task worktree**: the write goes
//!   through the broker and permission engine and is reviewed and merged in
//!   the existing diff reviewer.
//! - Convert and "Implement this" also land as tasks in worktrees.

#![allow(
    clippy::needless_pass_by_value, // Tauri commands take owned args
    clippy::missing_errors_doc,
    clippy::unused_self,
    clippy::similar_names,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::single_match_else
)]
use crate::service::{AppService, Sink, TaskRequest};
use crate::tasks::Script;
use crate::wire::StreamMessage;
use next_agent_core::llm::{ChatMessage, ChatRequest, CompletionClient};
use next_agent_core::store::DesignDirectionRow;
use next_agent_core::{AgentStep, EventKind, ToolRequest};
use next_agent_design::codemod::{self, StyleChange, Syntax};
use next_agent_design::convert::{self, Framework};
use next_agent_design::critique::Critique;
use next_agent_design::lint::Finding;
use next_agent_design::mock::MockDesignModel;
use next_agent_design::proxy::{self, PreviewProxy};
use next_agent_design::sandbox::{self, FrameKind};
use next_agent_design::source_map::{self, RawLocation};
use next_agent_design::studio::{self, DesignModel, Direction, Progress, Turn};
use next_agent_design::{prompts, sketch, ssrf, system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{Manager, State};

const MAX_DOC_BYTES: usize = 256 * 1024;
const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const HISTORY_LIMIT: u32 = 60;
const LOG_LINES: usize = 200;

// ------------------------------------------------------------------ models --

/// The gateway's `design` role as a [`DesignModel`].
struct GatewayModel {
    gateway: next_agent_core::router::Gateway,
}

impl DesignModel for GatewayModel {
    fn complete(&self, system: &str, turns: &[Turn]) -> Result<String, String> {
        let mut messages = vec![ChatMessage::system(system)];
        for turn in turns {
            messages.push(if turn.role == "assistant" {
                ChatMessage::assistant(Some(turn.text.clone()), Vec::new())
            } else {
                ChatMessage::user(turn.text.clone())
            });
        }
        let request = ChatRequest {
            model: String::new(), // resolved from the `design` role
            messages,
            tools: Vec::new(),
        };
        self.gateway
            .complete(&request, &mut |_| {})
            .map(|c| c.content)
            .map_err(|e| e.to_string())
    }
}

/// Which model answered, for the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub label: String,
    pub mock: bool,
    /// Why the mock is used, when it is.
    pub reason: Option<String>,
}

// -------------------------------------------------------------------- DTOs --

/// `DESIGN.md` + `tokens.css` of a workspace.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesignFiles {
    /// Workspace-relative paths.
    pub design_path: String,
    pub tokens_path: String,
    pub design_md: String,
    pub tokens_css: String,
    /// False when nothing is saved yet (the texts are a fresh draft).
    pub saved: bool,
    pub tokens: Vec<system::Token>,
    pub sources: Vec<String>,
}

/// A direction for the UI, with its ready-to-use sandboxed `srcdoc`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectionDto {
    pub id: String,
    pub batch_id: String,
    pub parent_id: Option<String>,
    pub prompt: String,
    pub frame: FrameKind,
    pub identifier: String,
    pub title: String,
    pub rationale: String,
    pub html: String,
    pub srcdoc: String,
    pub critique: Option<Critique>,
    pub lint: Vec<Finding>,
    pub note: String,
    pub reply: String,
    pub chosen: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesignBatch {
    pub batch_id: String,
    pub directions: Vec<DirectionDto>,
    pub model: ModelInfo,
    pub notices: Vec<String>,
}

/// A running edit-mode preview.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDto {
    /// URL to load in the iframe (the loopback proxy).
    pub url: String,
    /// The proxy's origin, for `postMessage` checks.
    pub origin: String,
    /// The dev server the proxy forwards to.
    pub target: String,
    /// The command started for the dev server, when the studio started one.
    pub command: Option<String>,
    pub sandbox: String,
}

/// "Jump to source": the clicked element's file and surrounding lines.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub tag: String,
    pub first_line: u32,
    pub lines: Vec<String>,
    pub syntax: Option<Syntax>,
}

/// A task the studio started.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesignTask {
    pub task_id: String,
    pub title: String,
    /// The file the task writes, when known.
    pub path: Option<String>,
}

// ------------------------------------------------------------ preview state --

struct DevServer {
    child: Child,
    command: String,
    log: Arc<Mutex<VecDeque<String>>>,
}

impl DevServer {
    fn stop(&mut self) {
        #[cfg(unix)]
        {
            // The dev server runs in its own process group: end the whole group.
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", self.child.id())])
                .status();
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Preview {
    /// Kept alive while the preview runs; dropping it stops the proxy.
    _proxy: PreviewProxy,
    dev: Option<DevServer>,
}

/// Managed Tauri state: one preview per workspace.
#[derive(Default)]
pub struct DesignState {
    previews: Mutex<HashMap<String, Preview>>,
}

impl DesignState {
    /// Stop every preview and dev server (called when the app exits:
    /// managed state is not dropped then).
    pub fn stop_all(&self) {
        if let Ok(mut previews) = self.previews.lock() {
            previews.clear();
        }
    }

    fn previews(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Preview>>, String> {
        self.previews
            .lock()
            .map_err(|_| "preview lock poisoned".to_string())
    }
}

// -------------------------------------------------------------- the service --

fn new_id(prefix: &str, n: usize) -> String {
    format!("{}{n}", crate::gateway::new_id(prefix))
}

/// Where a workspace's design files live: the folder holding `DESIGN.md`
/// (workspace root, then `design/`), or `design/` for a new one.
#[must_use]
pub fn design_dir(root: &Path) -> PathBuf {
    if root.join("DESIGN.md").is_file() {
        root.to_path_buf()
    } else {
        root.join("design")
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read a design file, refusing symlinks (they could point outside the
/// workspace) and oversized files.
fn read_doc(path: &Path) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok(None),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            Err(format!("{} is not a regular file", path.display()))
        }
        Ok(meta) if meta.len() > MAX_DOC_BYTES as u64 => Err(format!(
            "{} is larger than {} KB",
            path.display(),
            MAX_DOC_BYTES / 1024
        )),
        Ok(_) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| e.to_string()),
    }
}

/// Write one of the two design files (fixed names only), atomically.
fn write_doc(dir: &Path, name: &str, text: &str) -> Result<(), String> {
    if text.len() > MAX_DOC_BYTES {
        return Err(format!("{name} is larger than {} KB", MAX_DOC_BYTES / 1024));
    }
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a symlink", dir.display()));
    }
    let path = dir.join(name);
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a symlink", path.display()));
    }
    // Atomic, fsynced, and `create_new` on the temp file: a symlink
    // planted at the temp path is removed, never followed.
    next_agent_graph::layout::write_atomic(&path, text).map_err(|e| e.to_string())
}

impl AppService {
    fn workspace_root(&self, workspace_id: &str) -> Result<(PathBuf, String), String> {
        let ws = self.workspace(workspace_id)?;
        Ok((PathBuf::from(ws.path), ws.name))
    }

    /// The `design` role's model, or the offline mock with the reason.
    fn design_model(&self) -> (Box<dyn DesignModel>, ModelInfo) {
        match self.role_gateway("design") {
            Ok(gateway) => {
                let label = gateway
                    .config()
                    .role_target("design")
                    .unwrap_or_else(|| "design role".into());
                (
                    Box::new(GatewayModel { gateway }),
                    ModelInfo {
                        label,
                        mock: false,
                        reason: None,
                    },
                )
            }
            Err(why) => (
                Box::new(MockDesignModel),
                ModelInfo {
                    label: "offline mock".into(),
                    mock: true,
                    reason: Some(why),
                },
            ),
        }
    }

    /// The workspace's design files, or a draft generated from its CSS when
    /// none are saved yet.
    ///
    /// # Errors
    /// Unknown workspace or unreadable files.
    pub fn design_system_get(&self, workspace_id: &str) -> Result<DesignFiles, String> {
        let (root, name) = self.workspace_root(workspace_id)?;
        let dir = design_dir(&root);
        let design = read_doc(&dir.join("DESIGN.md"))?;
        let tokens = read_doc(&dir.join("tokens.css"))?;
        match (design, tokens) {
            (Some(design_md), Some(tokens_css)) => {
                let parsed = system::extract_from_sources(
                    &name,
                    &[("tokens.css".into(), tokens_css.clone())],
                );
                Ok(DesignFiles {
                    design_path: rel(&root, &dir.join("DESIGN.md")),
                    tokens_path: rel(&root, &dir.join("tokens.css")),
                    design_md,
                    tokens_css,
                    saved: true,
                    tokens: parsed.tokens,
                    sources: Vec::new(),
                })
            }
            (design, tokens) => {
                let mut draft = self.design_system_draft(&root, &name)?;
                draft.saved = design.is_some() || tokens.is_some();
                if let Some(d) = design {
                    draft.design_md = d;
                }
                if let Some(t) = tokens {
                    draft.tokens_css = t;
                }
                Ok(draft)
            }
        }
    }

    fn design_system_draft(&self, root: &Path, name: &str) -> Result<DesignFiles, String> {
        let extracted = system::extract(root, name)?;
        let dir = design_dir(root);
        Ok(DesignFiles {
            design_path: rel(root, &dir.join("DESIGN.md")),
            tokens_path: rel(root, &dir.join("tokens.css")),
            design_md: system::render_design_md(&extracted),
            tokens_css: system::render_tokens_css(&extracted),
            saved: false,
            tokens: extracted.tokens,
            sources: extracted.sources,
        })
    }

    /// "Generate from project": read the CSS and write both files.
    ///
    /// # Errors
    /// Unknown workspace, unreadable project, or files exist and
    /// `overwrite` is false.
    pub fn design_system_generate(
        &self,
        workspace_id: &str,
        overwrite: bool,
    ) -> Result<DesignFiles, String> {
        let (root, name) = self.workspace_root(workspace_id)?;
        let dir = design_dir(&root);
        if !overwrite && (dir.join("DESIGN.md").exists() || dir.join("tokens.css").exists()) {
            return Err(
                "exists: DESIGN.md or tokens.css is already saved; confirm to replace them".into(),
            );
        }
        let mut draft = self.design_system_draft(&root, &name)?;
        write_doc(&dir, "DESIGN.md", &draft.design_md)?;
        write_doc(&dir, "tokens.css", &draft.tokens_css)?;
        draft.saved = true;
        Ok(draft)
    }

    /// Save edited design files.
    ///
    /// # Errors
    /// Unknown workspace, oversized text or write failures.
    pub fn design_system_save(
        &self,
        workspace_id: &str,
        design_md: &str,
        tokens_css: &str,
    ) -> Result<DesignFiles, String> {
        let (root, _) = self.workspace_root(workspace_id)?;
        let dir = design_dir(&root);
        write_doc(&dir, "DESIGN.md", design_md)?;
        write_doc(&dir, "tokens.css", tokens_css)?;
        self.design_system_get(workspace_id)
    }

    fn dto(&self, row: &DesignDirectionRow, tokens_css: &str) -> Option<DirectionDto> {
        let d: Direction = serde_json::from_str(&row.body).ok()?;
        Some(DirectionDto {
            id: row.id.clone(),
            batch_id: row.batch_id.clone(),
            parent_id: row.parent_id.clone(),
            prompt: row.prompt.clone(),
            frame: FrameKind::parse(&row.frame),
            identifier: d.identifier,
            title: d.title,
            rationale: d.rationale,
            srcdoc: sandbox::sandbox_document(&d.html, tokens_css),
            html: d.html,
            critique: d.critique,
            lint: d.lint,
            note: row.note.clone(),
            reply: row.reply.clone(),
            chosen: row.chosen,
            created_at: row.created_at.clone(),
        })
    }

    fn frame_name(frame: FrameKind) -> &'static str {
        match frame {
            FrameKind::Web => "web",
            FrameKind::Mobile => "mobile",
            FrameKind::Desktop => "desktop",
        }
    }

    fn save_direction(
        &self,
        row: DesignDirectionRow,
        tokens_css: &str,
    ) -> Result<DirectionDto, String> {
        self.store()?
            .design_direction_insert(&row)
            .map_err(|e| e.to_string())?;
        let stored = self
            .store()?
            .design_direction(&row.id)
            .map_err(|e| e.to_string())?
            .ok_or("direction vanished")?;
        self.dto(&stored, tokens_css)
            .ok_or_else(|| "could not read the stored direction".into())
    }

    /// Generate 3–5 directions (with self-critique) and store them.
    ///
    /// # Errors
    /// Unknown workspace, board or model failure.
    #[allow(clippy::too_many_arguments)]
    pub fn design_generate(
        &self,
        workspace_id: &str,
        prompt: &str,
        frame: FrameKind,
        count: usize,
        board: Option<&str>,
        critique: bool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<DesignBatch, String> {
        if prompt.trim().is_empty() {
            return Err("describe the screen to design first".into());
        }
        let files = self.design_system_get(workspace_id)?;
        let mut notices = Vec::new();
        if !files.saved {
            notices.push("No DESIGN.md yet: used a draft generated from the project's CSS. Save it in the design-system panel to keep it.".to_string());
        }
        let sketch_text = match board.filter(|b| !b.trim().is_empty()) {
            Some(name) => {
                let vault = self
                    .vault_get()?
                    .ok_or("set a vault folder in Settings to use a board sketch")?;
                let board = crate::boards::sketch(Path::new(&vault), name)?;
                Some(sketch::describe(&board.elements))
            }
            None => None,
        };
        let (model, info) = self.design_model();
        if let Some(reason) = &info.reason {
            notices.push(format!(
                "Using the offline mock design model because {reason}. Assign a model to the design role in Settings → Providers."
            ));
        }
        let request = studio::GenerateRequest {
            prompt,
            frame,
            count,
            design_md: &files.design_md,
            tokens_css: &files.tokens_css,
            sketch: sketch_text.as_deref(),
            critique,
        };
        let directions = studio::generate(model.as_ref(), &request, progress)?;
        let batch_id = new_id("batch", 0);
        let mut out = Vec::new();
        for (i, d) in directions.into_iter().enumerate() {
            let row = DesignDirectionRow {
                id: new_id("dir", i),
                workspace_id: workspace_id.to_string(),
                batch_id: batch_id.clone(),
                parent_id: None,
                prompt: prompt.to_string(),
                frame: Self::frame_name(frame).into(),
                title: d.title.clone(),
                body: serde_json::to_string(&d).map_err(|e| e.to_string())?,
                note: String::new(),
                reply: String::new(),
                chosen: false,
                created_at: String::new(),
            };
            out.push(self.save_direction(row, &files.tokens_css)?);
        }
        Ok(DesignBatch {
            batch_id,
            directions: out,
            model: info,
            notices,
        })
    }

    /// Recent directions of a workspace, newest first.
    ///
    /// # Errors
    /// Unknown workspace or database errors.
    pub fn design_history(&self, workspace_id: &str) -> Result<Vec<DirectionDto>, String> {
        let files = self.design_system_get(workspace_id)?;
        let rows = self
            .store()?
            .design_directions(workspace_id, HISTORY_LIMIT)
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .filter_map(|r| self.dto(r, &files.tokens_css))
            .collect())
    }

    fn direction_row(&self, id: &str) -> Result<DesignDirectionRow, String> {
        self.store()?
            .design_direction(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("unknown direction {id}"))
    }

    /// A stored direction with its workspace's design files.
    fn load_direction(
        &self,
        id: &str,
    ) -> Result<(DesignDirectionRow, DesignFiles, Direction), String> {
        let row = self.direction_row(id)?;
        let files = self.design_system_get(&row.workspace_id)?;
        let direction = serde_json::from_str(&row.body).map_err(|e| e.to_string())?;
        Ok((row, files, direction))
    }

    /// Mark a direction as the picked one in its batch.
    ///
    /// # Errors
    /// Database errors.
    pub fn design_choose(&self, id: &str) -> Result<(), String> {
        self.store()?
            .design_direction_choose(id)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Refine a direction by chat; the result is a new version.
    ///
    /// # Errors
    /// Unknown direction or model failure.
    pub fn design_iterate(&self, id: &str, message: &str) -> Result<DirectionDto, String> {
        if message.trim().is_empty() {
            return Err("say what to change".into());
        }
        let (row, files, current) = self.load_direction(id)?;
        // Earlier refinements of this lineage, oldest first (at most 6).
        let mut history = Vec::new();
        let mut cursor = Some(row.clone());
        while let Some(r) = cursor {
            if !r.note.is_empty() {
                history.push(Turn::assistant(r.reply.clone()));
                history.push(Turn::user(r.note.clone()));
            }
            cursor = match &r.parent_id {
                Some(p) if history.len() < 12 => self.direction_row(p).ok(),
                _ => None,
            };
        }
        history.reverse();
        let frame = FrameKind::parse(&row.frame);
        let (model, _) = self.design_model();
        let (next, summary) = studio::iterate(
            model.as_ref(),
            &studio::IterateRequest {
                design_md: &files.design_md,
                tokens_css: &files.tokens_css,
                frame,
                current: &current,
                history: &history,
                message,
            },
        )?;
        let new_row = DesignDirectionRow {
            id: new_id("dir", 0),
            parent_id: Some(row.id.clone()),
            body: serde_json::to_string(&next).map_err(|e| e.to_string())?,
            note: message.to_string(),
            reply: summary,
            chosen: false,
            created_at: String::new(),
            ..row
        };
        let saved = self.save_direction(new_row, &files.tokens_css)?;
        self.design_choose(&saved.id)?;
        Ok(DirectionDto {
            chosen: true,
            ..saved
        })
    }

    /// Start the edit-mode preview: the workspace's dev server (started
    /// here when `url` is `None`) behind the loopback proxy.
    ///
    /// # Errors
    /// Unknown workspace, a non-loopback URL, no dev script, or the dev
    /// server could not be started.
    pub fn design_preview_start(
        &self,
        state: &DesignState,
        workspace_id: &str,
        url: Option<&str>,
        studio_origin: &str,
    ) -> Result<PreviewDto, String> {
        let (root, _) = self.workspace_root(workspace_id)?;
        let bridge = proxy::bridge_script(studio_origin)?;
        let (target, dev) = match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(url) => (ssrf::check_preview_url(url)?, None),
            None => {
                // Stop the old preview first: it may be this project's dev server.
                state.previews()?.remove(workspace_id);
                let dev = start_dev_server(&root)?;
                let target = ssrf::check_preview_url(&format!("http://localhost:{}/", dev.1))?;
                (target, Some(dev.0))
            }
        };
        let target_text = format!("http://{}{}", target.authority(), target.path);
        let proxy = PreviewProxy::start(target, bridge).map_err(|e| e.to_string())?;
        // One preview per workspace: the new one replaces any running one.
        state.previews()?.remove(workspace_id);
        let dto = PreviewDto {
            url: proxy.url(),
            origin: proxy.origin(),
            target: target_text,
            command: dev.as_ref().map(|d| d.command.clone()),
            sandbox: sandbox::APP_SANDBOX.into(),
        };
        state
            .previews()?
            .insert(workspace_id.to_string(), Preview { _proxy: proxy, dev });
        Ok(dto)
    }

    /// Resolve a clicked element to its source and the lines around it.
    ///
    /// # Errors
    /// The location is outside the workspace or not a source file.
    pub fn design_source(
        &self,
        workspace_id: &str,
        location: &RawLocation,
    ) -> Result<SourceView, String> {
        let (root, _) = self.workspace_root(workspace_id)?;
        let loc = source_map::resolve(&root, location)?;
        let text = read_source(&root, &loc.file)?;
        let (first_line, lines) = source_map::snippet(&text, loc.line.max(1), 8);
        Ok(SourceView {
            syntax: Syntax::for_path(&loc.file),
            file: loc.file,
            line: loc.line,
            column: loc.column,
            tag: loc.tag,
            first_line,
            lines,
        })
    }

    /// Compute a style edit (without writing) so the UI can show the change.
    fn style_edit(
        root: &Path,
        location: &RawLocation,
        changes: &[StyleChange],
    ) -> Result<(String, String, codemod::StyleEdit), String> {
        let loc = source_map::resolve(root, location)?;
        if loc.line == 0 {
            return Err(format!(
                "{} gives no line number (Vue without the instrument plugin); add `nextAgentSource()` from @next-agent/design-instrument to vite.config to edit visually",
                loc.file
            ));
        }
        let syntax =
            Syntax::for_path(&loc.file).ok_or("this file type can't be edited visually")?;
        let seen = read_source(root, &loc.file)?;
        let edit = codemod::apply_style(&seen, syntax, loc.line, loc.column, &loc.tag, changes)?;
        Ok((loc.file, seen, edit))
    }

    /// Write a previewed style change to the code: a scripted task in a task
    /// worktree whose single `write_file` goes through the broker and the
    /// permission engine. Review and merge it like any task.
    ///
    /// # Errors
    /// Bad location or style, no git worktree, or the task could not start.
    pub fn design_apply_style(
        self: &Arc<Self>,
        workspace_id: &str,
        location: &RawLocation,
        changes: &[StyleChange],
        sink: Sink,
    ) -> Result<DesignTask, String> {
        let (root, _) = self.workspace_root(workspace_id)?;
        let (file, seen, edit) = Self::style_edit(&root, location, changes)?;
        let summary = changes
            .iter()
            .map(|c| {
                if c.value.is_empty() {
                    format!("remove {}", c.property)
                } else {
                    format!("{}: {}", c.property, c.value)
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        let title = format!(
            "Design edit: <{}> in {file}",
            location.tag.to_ascii_lowercase()
        );
        let prompt = format!(
            "{title}\n\n{summary}\n\nBefore: {}\nAfter:  {}",
            edit.before, edit.after
        );
        let target = file.clone();
        let script: Script = Box::new(move |sandbox: &Path| {
            // The worktree's copy may differ from the dev server's working copy.
            if !sandbox.join(&target).is_file() {
                return Err(format!(
                    "{target} is not committed, so the task worktree has no copy of it; commit it first"
                ));
            }
            let current = read_source(sandbox, &target)?;
            let contents = codemod::transplant(&seen, &current, &edit)?;
            Ok(vec![
                AgentStep::Emit(EventKind::Message {
                    text: format!(
                        "Design Studio: writing the visual edit to {target} ({summary})."
                    ),
                }),
                AgentStep::Call {
                    call_id: "design_edit".into(),
                    request: ToolRequest::Write {
                        path: sandbox.join(&target),
                        contents,
                    },
                },
            ])
        });
        let task_id =
            self.task_start_scripted(&design_request(workspace_id, prompt), sink, script)?;
        Ok(DesignTask {
            task_id,
            title,
            path: Some(file),
        })
    }

    /// Convert a direction to the project's framework (or `framework`) and
    /// land the file as a task in a worktree.
    ///
    /// # Errors
    /// Unknown direction, model failure, or the task could not start.
    pub fn design_convert(
        self: &Arc<Self>,
        id: &str,
        framework: Option<&str>,
        sink: Sink,
    ) -> Result<DesignTask, String> {
        let (row, files, direction) = self.load_direction(id)?;
        let (root, _) = self.workspace_root(&row.workspace_id)?;
        let framework = framework
            .and_then(Framework::parse)
            .unwrap_or_else(|| convert::detect(&root));
        let (model, _) = self.design_model();
        let mut converted = studio::convert(
            model.as_ref(),
            &root,
            &direction,
            framework,
            &files.design_md,
        )?;
        // Never overwrite an existing file: pick a free name.
        let mut n = 2;
        let original = converted.path.clone();
        while root.join(&converted.path).exists() {
            let (stem, ext) = original.rsplit_once('.').unwrap_or((&original, ""));
            converted.path = format!("{stem}{n}.{ext}");
            n += 1;
        }
        let path = converted.path.clone();
        let title = format!("Design convert: \"{}\" to {}", direction.title, path);
        let prompt = format!(
            "{title}\n\nConverted from the Design Studio direction {id} ({:?}).",
            framework
        );
        let script: Script = Box::new(move |sandbox: &Path| {
            Ok(vec![
                AgentStep::Emit(EventKind::Message {
                    text: format!(
                        "Design Studio: adding {} ({:?}).",
                        converted.path, converted.framework
                    ),
                }),
                AgentStep::Call {
                    call_id: "design_convert".into(),
                    request: ToolRequest::Write {
                        path: sandbox.join(&converted.path),
                        contents: converted.contents,
                    },
                },
            ])
        });
        let task_id =
            self.task_start_scripted(&design_request(&row.workspace_id, prompt), sink, script)?;
        Ok(DesignTask {
            task_id,
            title,
            path: Some(path),
        })
    }

    /// "Implement this": hand the direction and DESIGN.md to the coding agent
    /// as an auto-mode task (worktree, review, merge).
    ///
    /// # Errors
    /// Unknown direction or the task could not start.
    pub fn design_implement(self: &Arc<Self>, id: &str, sink: Sink) -> Result<DesignTask, String> {
        let (row, files, direction) = self.load_direction(id)?;
        let prompt = prompts::implement_task(
            &direction.title,
            &row.prompt,
            FrameKind::parse(&row.frame),
            &direction.html,
            &files.design_path,
        );
        let title: String = prompt
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(60)
            .collect();
        let task_id = self.task_start(&design_request(&row.workspace_id, prompt), sink)?;
        Ok(DesignTask {
            task_id,
            title,
            path: None,
        })
    }
}

fn design_request(workspace_id: &str, prompt: String) -> TaskRequest {
    TaskRequest {
        workspace_id: workspace_id.to_string(),
        prompt,
        mode: "auto".into(),
        profile: None,
    }
}

/// Read a workspace source file after the same checks edit mode uses.
fn read_source(root: &Path, rel: &str) -> Result<String, String> {
    let real = source_map::check_source_file(root, rel)?;
    let size = std::fs::metadata(&real).map_err(|e| e.to_string())?.len();
    if size > MAX_SOURCE_BYTES {
        return Err(format!("{rel} is too large to edit visually"));
    }
    std::fs::read_to_string(&real).map_err(|e| e.to_string())
}

/// The package manager a project uses, from its lockfile.
fn package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    }
}

/// The dev command for a project: `(program, args)` with `--port <port>`.
///
/// # Errors
/// No `package.json` or no `dev` script.
pub fn dev_command(root: &Path, port: u16) -> Result<(String, Vec<String>), String> {
    let text = std::fs::read_to_string(root.join("package.json")).map_err(|_| {
        "no package.json: start the dev server yourself and enter its URL".to_string()
    })?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Some(script) = json.pointer("/scripts/dev").and_then(|v| v.as_str()) else {
        return Err(
            "package.json has no \"dev\" script: start the dev server yourself and enter its URL"
                .into(),
        );
    };
    let pm = package_manager(root);
    let mut args = vec!["run".to_string(), "dev".to_string()];
    if pm == "npm" {
        args.push("--".into());
    }
    args.extend(["--port".to_string(), port.to_string()]);
    // Vite would silently move to another port if ours were taken; another
    // local process could then be served into the preview.
    if script.contains("vite") {
        args.push("--strictPort".into());
    }
    Ok((pm.to_string(), args))
}

/// Strip ANSI colour codes from a log line.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// PATH with the usual per-user tool folders (a GUI app starts with a
/// minimal PATH on macOS).
fn tool_path() -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for extra in [
            ".local/bin",
            "Library/pnpm",
            ".bun/bin",
            ".volta/bin",
            ".cargo/bin",
        ] {
            dirs.push(home.join(extra));
        }
    }
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

fn start_dev_server(root: &Path) -> Result<(DevServer, u16), String> {
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let (program, args) = dev_command(root, port)?;
    let mut command = Command::new(&program);
    command
        .args(&args)
        .current_dir(root)
        .env("PATH", tool_path())
        .env("PORT", port.to_string())
        .env("BROWSER", "none")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        format!(
            "could not start `{program}` ({e}); start the dev server yourself and enter its URL"
        )
    })?;
    let log: Arc<Mutex<VecDeque<String>>> = Arc::default();
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let log = Arc::clone(&log);
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if let Ok(mut log) = log.lock() {
                    if log.len() >= LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(strip_ansi(&line).chars().take(500).collect());
                }
            }
        });
    }
    let command = format!("{program} {}", args.join(" "));
    Ok((
        DevServer {
            child,
            command,
            log,
        },
        port,
    ))
}

// ---------------------------------------------------------------- commands --

type Svc<'a> = State<'a, Arc<AppService>>;
type Design<'a> = State<'a, DesignState>;

fn announce(app: &tauri::AppHandle, task: &DesignTask) {
    crate::commands::announce_task(app, &task.task_id, &task.title, "auto");
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_system_get(service: Svc, workspace_id: String) -> Result<DesignFiles, String> {
    service.design_system_get(&workspace_id)
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_system_generate(
    service: Svc,
    workspace_id: String,
    overwrite: bool,
) -> Result<DesignFiles, String> {
    service.design_system_generate(&workspace_id, overwrite)
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_system_save(
    service: Svc,
    workspace_id: String,
    design_md: String,
    tokens_css: String,
) -> Result<DesignFiles, String> {
    service.design_system_save(&workspace_id, &design_md, &tokens_css)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn design_generate(
    service: Svc<'_>,
    workspace_id: String,
    prompt: String,
    frame: String,
    count: usize,
    board: Option<String>,
    critique: bool,
    on_progress: Channel<Progress>,
) -> Result<DesignBatch, String> {
    let service = Arc::clone(&service);
    blocking(move || {
        service.design_generate(
            &workspace_id,
            &prompt,
            FrameKind::parse(&frame),
            count,
            board.as_deref(),
            critique,
            &mut |p| {
                let _ = on_progress.send(p);
            },
        )
    })
    .await
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_history(service: Svc, workspace_id: String) -> Result<Vec<DirectionDto>, String> {
    service.design_history(&workspace_id)
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_choose(service: Svc, id: String) -> Result<(), String> {
    service.design_choose(&id)
}

#[tauri::command]
pub async fn design_iterate(
    service: Svc<'_>,
    id: String,
    message: String,
) -> Result<DirectionDto, String> {
    let service = Arc::clone(&service);
    blocking(move || service.design_iterate(&id, &message)).await
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_preview_start(
    service: Svc,
    state: Design,
    workspace_id: String,
    url: Option<String>,
    studio_origin: String,
) -> Result<PreviewDto, String> {
    service.design_preview_start(&state, &workspace_id, url.as_deref(), &studio_origin)
}

#[tauri::command]
pub fn design_preview_stop(state: Design, workspace_id: String) -> Result<(), String> {
    state.previews()?.remove(&workspace_id);
    Ok(())
}

#[tauri::command]
pub fn design_preview_log(state: Design, workspace_id: String) -> Result<Vec<String>, String> {
    Ok(state
        .previews()?
        .get(&workspace_id)
        .and_then(|p| p.dev.as_ref())
        .and_then(|d| d.log.lock().ok().map(|l| l.iter().cloned().collect()))
        .unwrap_or_default())
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_source(
    service: Svc,
    workspace_id: String,
    location: RawLocation,
) -> Result<SourceView, String> {
    service.design_source(&workspace_id, &location)
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_apply_style(
    app: tauri::AppHandle,
    service: Svc,
    workspace_id: String,
    location: RawLocation,
    changes: Vec<StyleChange>,
    on_event: Channel<StreamMessage>,
) -> Result<DesignTask, String> {
    let task = Arc::clone(&service).design_apply_style(
        &workspace_id,
        &location,
        &changes,
        island_sink(&app, on_event),
    )?;
    announce(&app, &task);
    Ok(task)
}

#[tauri::command]
pub async fn design_convert(
    app: tauri::AppHandle,
    service: Svc<'_>,
    id: String,
    framework: Option<String>,
    on_event: Channel<StreamMessage>,
) -> Result<DesignTask, String> {
    let service = Arc::clone(&service);
    let sink = island_sink(&app, on_event);
    let task = blocking(move || service.design_convert(&id, framework.as_deref(), sink)).await?;
    announce(&app, &task);
    Ok(task)
}

// Off the main thread: disk walks, git worktrees, process spawns.
#[tauri::command(async)]
pub fn design_implement(
    app: tauri::AppHandle,
    service: Svc,
    id: String,
    on_event: Channel<StreamMessage>,
) -> Result<DesignTask, String> {
    let task = Arc::clone(&service).design_implement(&id, island_sink(&app, on_event))?;
    announce(&app, &task);
    Ok(task)
}

/// A Design Studio task's event sink: the caller's Channel, also shown on
/// Coucou's island like any other task.
fn island_sink(app: &tauri::AppHandle, on_event: Channel<StreamMessage>) -> Sink {
    crate::commands::task_sink(
        &app.state::<crate::coucou::Adapter>(),
        on_event,
        crate::coucou::TaskContext {
            prompt: "Design Studio".into(),
            cwd: String::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as Cmd;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Cmd::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A committed Svelte project + a service with it opened.
    fn project() -> (tempfile::TempDir, Arc<AppService>, String, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("app");
        std::fs::create_dir_all(root.join("src/routes")).expect("mk");
        std::fs::write(
            root.join("src/app.css"),
            ":root { --bg: #fafafa; --accent: #3b5bdb; }\n@media (prefers-color-scheme: dark) { :root { --bg: #111; } }\n",
        )
        .expect("css");
        std::fs::write(
            root.join("src/routes/+page.svelte"),
            "<main class=\"page\">\n  <h1>Hello</h1>\n</main>\n",
        )
        .expect("page");
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"vite dev"},"devDependencies":{"@sveltejs/kit":"2"}}"#,
        )
        .expect("pkg");
        std::fs::create_dir_all(root.join("src/lib")).expect("lib");
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "init"]);
        let svc = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("svc"));
        let ws = svc
            .workspace_open(root.to_str().expect("utf8"))
            .expect("ws");
        (dir, svc, ws.id, root)
    }

    fn collect() -> (Sink, Arc<Mutex<Vec<StreamMessage>>>) {
        let seen: Arc<Mutex<Vec<StreamMessage>>> = Arc::default();
        let s = Arc::clone(&seen);
        (Arc::new(move |m| s.lock().expect("lock").push(m)), seen)
    }

    fn event_kinds(seen: &Arc<Mutex<Vec<StreamMessage>>>) -> Vec<serde_json::Value> {
        seen.lock()
            .expect("lock")
            .iter()
            .filter_map(|m| match m {
                StreamMessage::Event { event } => Some(event.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn design_system_draft_generate_and_save() {
        let (_d, svc, ws, root) = project();
        let draft = svc.design_system_get(&ws).expect("draft");
        assert!(!draft.saved);
        assert_eq!(draft.design_path, "design/DESIGN.md");
        assert!(draft.tokens_css.contains("--accent: #3b5bdb;"));
        assert!(draft.tokens_css.contains("--bg: #111;"), "dark value");
        let generated = svc.design_system_generate(&ws, false).expect("generate");
        assert!(generated.saved);
        assert!(root.join("design/DESIGN.md").is_file());
        assert!(svc
            .design_system_generate(&ws, false)
            .unwrap_err()
            .starts_with("exists:"));
        let saved = svc
            .design_system_save(&ws, "# Edited", ":root { --accent: red; }")
            .expect("save");
        assert_eq!(saved.design_md, "# Edited");
        assert_eq!(saved.tokens[0].name, "--accent");
    }

    #[cfg(unix)]
    #[test]
    fn design_files_refuse_symlinks() {
        let (_d, svc, ws, root) = project();
        let outside = tempfile::tempdir().expect("tmp");
        std::os::unix::fs::symlink(outside.path(), root.join("design")).expect("ln");
        assert!(svc
            .design_system_save(&ws, "x", "y")
            .unwrap_err()
            .contains("symlink"));
        assert!(!outside.path().join("DESIGN.md").exists());
    }

    #[test]
    fn generate_iterate_choose_with_the_mock_model() {
        let (_d, svc, ws, _root) = project();
        let mut phases = Vec::new();
        let batch = svc
            .design_generate(
                &ws,
                "Team settings",
                FrameKind::Desktop,
                3,
                None,
                true,
                &mut |p| {
                    phases.push(p);
                },
            )
            .expect("generate");
        assert!(batch.model.mock, "no provider configured in tests");
        assert_eq!(batch.directions.len(), 3);
        assert!(batch.notices.iter().any(|n| n.contains("offline mock")));
        let first = &batch.directions[0];
        assert!(first.srcdoc.contains(sandbox::DIRECTION_CSP));
        assert!(first.critique.is_some());
        assert_eq!(phases.last(), Some(&Progress::Done));

        let v2 = svc
            .design_iterate(&first.id, "tighter header")
            .expect("iterate");
        assert_eq!(v2.parent_id.as_deref(), Some(first.id.as_str()));
        assert!(v2.chosen);
        assert!(v2.html.contains("Revised: tighter header"));
        let history = svc.design_history(&ws).expect("history");
        assert_eq!(history.len(), 4);
        assert_eq!(history.iter().filter(|d| d.chosen).count(), 1);
        assert!(svc
            .design_generate(&ws, "  ", FrameKind::Web, 3, None, false, &mut |_| {})
            .is_err());
    }

    #[test]
    fn style_edits_go_through_a_scripted_worktree_task() {
        let (_d, svc, ws, root) = project();
        let location = RawLocation {
            origin: source_map::Origin::Svelte,
            file: "src/routes/+page.svelte".into(),
            line: 2,
            column: 2,
            tag: "h1".into(),
        };
        let view = svc.design_source(&ws, &location).expect("source");
        assert_eq!((view.line, view.column), (2, 3));
        assert_eq!(view.lines[1], "  <h1>Hello</h1>");
        let (sink, seen) = collect();
        let task = svc
            .design_apply_style(
                &ws,
                &location,
                &[StyleChange {
                    property: "color".into(),
                    value: "var(--accent)".into(),
                }],
                sink,
            )
            .expect("task");
        svc.task_wait(&task.task_id);
        // The workspace itself is untouched until the user merges.
        let page = std::fs::read_to_string(root.join("src/routes/+page.svelte")).expect("read");
        assert!(!page.contains("style:color"));
        // The write went through the broker: a tool call + result event.
        let events = event_kinds(&seen);
        let call = events
            .iter()
            .find(|e| e["kind"] == "tool_requested")
            .expect("tool_requested event");
        assert_eq!(call["data"]["tool"], "write_file", "{call}");
        // …the permission engine decided it…
        assert!(
            events.iter().any(|e| e["kind"] == "permission_decided"
                && e["data"]["call_id"] == "design_edit"
                && e["data"]["decision"] == "allow"),
            "{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| e["kind"] == "tool_finished" && e["data"]["ok"] == true),
            "{events:?}"
        );
        // …and produced a reviewable diff in a worktree branch.
        let review = seen
            .lock()
            .expect("lock")
            .iter()
            .find_map(|m| match m {
                StreamMessage::Review { files, .. } => {
                    Some(serde_json::to_string(files).expect("json"))
                }
                _ => None,
            })
            .expect("review");
        assert!(
            review.contains("+page.svelte") && review.contains("style:color"),
            "{review}"
        );
        let outcome = svc.review_action(&task.task_id, "merge").expect("merge");
        assert_eq!(outcome.files.len(), 1);
        let merged = std::fs::read_to_string(root.join("src/routes/+page.svelte")).expect("read");
        assert!(
            merged.contains("<h1 style:color=\"var(--accent)\">Hello</h1>"),
            "{merged}"
        );
    }

    #[test]
    fn style_edits_refuse_bad_input_before_any_task() {
        let (_d, svc, ws, _root) = project();
        let (sink, _) = collect();
        let outside = RawLocation {
            origin: source_map::Origin::Attr,
            file: "../../etc/passwd".into(),
            line: 1,
            column: 1,
            tag: "p".into(),
        };
        let change = [StyleChange {
            property: "color".into(),
            value: "red".into(),
        }];
        assert!(svc
            .design_apply_style(&ws, &outside, &change, Arc::clone(&sink))
            .is_err());
        let ok_loc = RawLocation {
            origin: source_map::Origin::Svelte,
            file: "src/routes/+page.svelte".into(),
            line: 2,
            column: 2,
            tag: "h1".into(),
        };
        let evil = [StyleChange {
            property: "color".into(),
            value: "red\" onclick=\"alert(1)".into(),
        }];
        assert!(svc.design_apply_style(&ws, &ok_loc, &evil, sink).is_err());
    }

    #[test]
    fn scripted_tasks_need_a_worktree() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("plain");
        std::fs::create_dir_all(root.join("src")).expect("mk");
        std::fs::write(root.join("src/a.html"), "<p>x</p>\n").expect("w");
        let svc = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("svc"));
        let ws = svc
            .workspace_open(root.to_str().expect("utf8"))
            .expect("ws");
        let (sink, _) = collect();
        let loc = RawLocation {
            origin: source_map::Origin::Attr,
            file: "src/a.html".into(),
            line: 1,
            column: 1,
            tag: "p".into(),
        };
        let err = svc
            .design_apply_style(
                &ws.id,
                &loc,
                &[StyleChange {
                    property: "color".into(),
                    value: "red".into(),
                }],
                sink,
            )
            .unwrap_err();
        assert!(err.contains("worktree"), "{err}");
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.html")).expect("r"),
            "<p>x</p>\n"
        );
    }

    #[test]
    fn convert_lands_a_new_file_in_a_worktree() {
        let (_d, svc, ws, root) = project();
        let batch = svc
            .design_generate(&ws, "Login", FrameKind::Mobile, 3, None, false, &mut |_| {})
            .expect("generate");
        let (sink, seen) = collect();
        let task = svc
            .design_convert(&batch.directions[0].id, None, sink)
            .expect("convert");
        assert_eq!(
            task.path.as_deref(),
            Some("src/lib/components/ModernMinimal.svelte")
        );
        svc.task_wait(&task.task_id);
        assert!(!root
            .join("src/lib/components/ModernMinimal.svelte")
            .exists());
        assert!(event_kinds(&seen)
            .iter()
            .any(|e| e["kind"] == "tool_requested" && e["data"]["tool"] == "write_file"));
        svc.review_action(&task.task_id, "merge").expect("merge");
        let code = std::fs::read_to_string(root.join("src/lib/components/ModernMinimal.svelte"))
            .expect("merged");
        assert!(code.contains("$props()"));
    }

    #[test]
    fn preview_refuses_remote_urls_and_bad_origins() {
        let (_d, svc, ws, _root) = project();
        let state = DesignState::default();
        for url in [
            "http://example.com",
            "http://169.254.169.254/",
            "https://localhost:5173",
        ] {
            assert!(
                svc.design_preview_start(&state, &ws, Some(url), "tauri://localhost")
                    .is_err(),
                "{url}"
            );
        }
        assert!(svc
            .design_preview_start(&state, &ws, Some("http://localhost:5173"), "x\";alert(1)//")
            .is_err());
        let ok = svc
            .design_preview_start(
                &state,
                &ws,
                Some("http://localhost:5173/app"),
                "tauri://localhost",
            )
            .expect("preview");
        assert!(ok.url.starts_with("http://127.0.0.1:") && ok.url.ends_with("/app"));
        assert_eq!(ok.target, "http://localhost:5173/app");
        assert_eq!(ok.sandbox, "allow-scripts allow-same-origin allow-forms");
    }

    #[test]
    fn dev_commands_per_package_manager() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(dev_command(dir.path(), 1).is_err());
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"x"}}"#,
        )
        .expect("w");
        assert!(dev_command(dir.path(), 1)
            .unwrap_err()
            .contains("no \"dev\" script"));
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .expect("w");
        assert_eq!(
            dev_command(dir.path(), 5000).expect("npm").1,
            vec!["run", "dev", "--", "--port", "5000", "--strictPort"]
        );
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").expect("w");
        let (pm, args) = dev_command(dir.path(), 5000).expect("pnpm");
        assert_eq!((pm.as_str(), args.len()), ("pnpm", 5));
        assert_eq!(
            strip_ansi("\u{1b}[32mready\u{1b}[0m in 3ms"),
            "ready in 3ms"
        );
    }
}
