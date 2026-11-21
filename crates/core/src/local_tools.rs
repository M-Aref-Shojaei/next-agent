//! The real [`ToolExecutor`] for the M1 tool set.
//!
//! Only ever invoked by the broker *after* the permission engine allowed the
//! call, so it performs no policy checks of its own (two layers that both
//! decide can disagree). It does enforce resource limits: output caps, a
//! command timeout, and killing the process group on cancel.
//!
//! `http_fetch` goes through the [`HttpFetch`] seam so HTTP stays inside the
//! router adapter (`crate::router`), per the module boundary rules.

use crate::broker::ToolExecutor;
use crate::edit_match;
use crate::events::{Emitter, EventKind};
use crate::runner::RunContext;
use crate::task::TaskId;
use crate::tools::{ToolRequest, ToolResult};
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A fetched HTTP resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedPage {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
    pub truncated: bool,
}

/// HTTP GET for the `http_fetch` tool. Implemented by
/// [`ReqwestFetcher`](crate::router::ReqwestFetcher).
pub trait HttpFetch: Send + Sync {
    /// Fetch `url`.
    ///
    /// # Errors
    /// Returns a human-readable message on connection or protocol failure.
    fn fetch(&self, url: &str) -> Result<FetchedPage, String>;
}

/// Resource limits for [`LocalTools`].
#[derive(Debug, Clone, Copy)]
pub struct ToolLimits {
    pub command_timeout: Duration,
    /// Cap on captured command output (both streams together).
    pub max_output_bytes: usize,
    pub max_read_bytes: u64,
    pub max_search_matches: usize,
    pub max_dir_entries: usize,
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            command_timeout: Duration::from_secs(120),
            max_output_bytes: 64 * 1024,
            max_read_bytes: 1024 * 1024,
            max_search_matches: 200,
            max_dir_entries: 1_000,
        }
    }
}

/// Executes M1 tools against the local machine.
#[derive(Clone, Default)]
pub struct LocalTools {
    limits: ToolLimits,
    http: Option<Arc<dyn HttpFetch>>,
}

impl std::fmt::Debug for LocalTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTools")
            .field("limits", &self.limits)
            .field("http", &self.http.is_some())
            .finish()
    }
}

impl LocalTools {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_limits(mut self, limits: ToolLimits) -> Self {
        self.limits = limits;
        self
    }

    #[must_use]
    pub fn with_http(mut self, http: Arc<dyn HttpFetch>) -> Self {
        self.http = Some(http);
        self
    }

    fn read_file(&self, call_id: &str, path: &Path) -> ToolResult {
        let size = match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                return ToolResult::error(call_id, format!("{} is a directory", path.display()))
            }
            Ok(meta) => meta.len(),
            Err(error) => return io_error(call_id, path, &error),
        };
        if size > self.limits.max_read_bytes {
            return ToolResult::error(
                call_id,
                format!(
                    "{} is {size} bytes; the read limit is {} bytes",
                    path.display(),
                    self.limits.max_read_bytes
                ),
            );
        }
        match fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => ToolResult::ok(call_id, text),
                Err(_) => {
                    ToolResult::error(call_id, format!("{} is not UTF-8 text", path.display()))
                }
            },
            Err(error) => io_error(call_id, path, &error),
        }
    }

    fn list_dir(&self, call_id: &str, path: &Path) -> ToolResult {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => return io_error(call_id, path, &error),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| {
                let mut name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    name.push('/');
                }
                name
            })
            .collect();
        names.sort();
        let total = names.len();
        names.truncate(self.limits.max_dir_entries);
        let mut output = names.join("\n");
        if total > self.limits.max_dir_entries {
            let _ = write!(
                output,
                "\n… {} more entries",
                total - self.limits.max_dir_entries
            );
        }
        ToolResult::ok(call_id, output)
    }

    fn search(
        &self,
        ctx: &RunContext<'_>,
        call_id: &str,
        pattern: &str,
        root: &Path,
    ) -> ToolResult {
        let matcher = match RegexMatcher::new_line_matcher(pattern) {
            Ok(matcher) => matcher,
            Err(error) => return ToolResult::error(call_id, format!("invalid pattern: {error}")),
        };
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(b'\x00'))
            .line_number(true)
            .build();
        let limit = self.limits.max_search_matches;
        let mut lines = Vec::new();
        let mut truncated = false;
        for entry in ignore::WalkBuilder::new(root).build() {
            if ctx.is_cancelled() {
                return ToolResult::error(call_id, "cancelled");
            }
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            let shown = path
                .strip_prefix(root)
                .ok()
                .filter(|rel| !rel.as_os_str().is_empty())
                .unwrap_or(path)
                .display()
                .to_string();
            let _ = searcher.search_path(
                &matcher,
                path,
                UTF8(|line_number, line| {
                    if lines.len() >= limit {
                        truncated = true;
                        return Ok(false);
                    }
                    lines.push(format!("{shown}:{line_number}:{}", line.trim_end()));
                    Ok(true)
                }),
            );
            if truncated {
                break;
            }
        }
        if lines.is_empty() {
            return ToolResult::ok(call_id, "no matches");
        }
        let mut output = lines.join("\n");
        if truncated {
            let _ = write!(output, "\n… stopped after {limit} matches");
        }
        ToolResult::ok(call_id, output)
    }

    fn write_file(call_id: &str, path: &Path, contents: &str) -> ToolResult {
        match write_atomic(path, contents) {
            Ok(()) => ToolResult::ok(
                call_id,
                format!("wrote {} bytes to {}", contents.len(), path.display()),
            ),
            Err(error) => io_error(call_id, path, &error),
        }
    }

    fn apply_patch(&self, call_id: &str, path: &Path, diff: &str) -> ToolResult {
        let original = match read_text(path, self.limits.max_read_bytes) {
            Ok(text) => text.unwrap_or_default(),
            Err(result) => return result.for_call(call_id),
        };
        let (bom, base) = edit_match::split_bom(&original);
        let patched = match edit_match::apply_unified_patch(base, diff) {
            Ok(patched) => patched,
            Err(error) => {
                let message = if error.starts_with("invalid patch") {
                    error
                } else {
                    format!("patch does not apply to {}: {error}", path.display())
                };
                return ToolResult::error(call_id, message);
            }
        };
        match write_atomic(path, &edit_match::join_bom(bom, &patched.text)) {
            Ok(()) => ToolResult::ok(
                call_id,
                format!(
                    "patched {} ({} hunk(s){})",
                    path.display(),
                    patched.hunks,
                    if patched.fuzzy { ", fuzzy match" } else { "" }
                ),
            ),
            Err(error) => io_error(call_id, path, &error),
        }
    }

    fn edit(
        &self,
        call_id: &str,
        path: &Path,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> ToolResult {
        let original = match read_text(path, self.limits.max_read_bytes) {
            Ok(Some(text)) => text,
            Ok(None) => {
                return ToolResult::error(
                    call_id,
                    format!(
                        "file not found: {}. Use write_file to create it.",
                        path.display()
                    ),
                )
            }
            Err(result) => return result.for_call(call_id),
        };
        let (bom, source) = edit_match::split_bom(&original);
        match edit_match::apply_edit(source, old_string, new_string, replace_all) {
            Ok(edited) => match write_atomic(path, &edit_match::join_bom(bom, &edited.text)) {
                Ok(()) => ToolResult::ok(
                    call_id,
                    format!(
                        "edited {} ({} replacement{})",
                        path.display(),
                        edited.replacements,
                        if edited.replacements == 1 { "" } else { "s" }
                    ),
                ),
                Err(error) => io_error(call_id, path, &error),
            },
            Err(error) => ToolResult::error(call_id, format!("{}: {error}", path.display())),
        }
    }

    fn run_command(
        &self,
        ctx: &RunContext<'_>,
        call_id: &str,
        command: &str,
        cwd: &Path,
    ) -> ToolResult {
        // Resolve and revalidate immediately before spawning: a path can be swapped after
        // request construction, so never pass an untrusted spelling to current_dir.
        let resolved_cwd = match cwd.canonicalize() {
            Ok(path) if path.is_dir() => path,
            Ok(path) => {
                return ToolResult::error(
                    call_id,
                    format!("exec cwd is not a directory: {}", path.display()),
                )
            }
            Err(error) => {
                return ToolResult::error(
                    call_id,
                    format!("could not resolve exec cwd {}: {error}", cwd.display()),
                )
            }
        };
        let mut child = match shell(command)
            .current_dir(&resolved_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return ToolResult::error(call_id, format!("could not start `{command}`: {error}"))
            }
        };

        let (sender, chunks) = mpsc::channel::<(&'static str, Vec<u8>)>();
        if let Some(stdout) = child.stdout.take() {
            pump("stdout", stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pump("stderr", stderr, sender.clone());
        }
        drop(sender);

        let deadline = Instant::now() + self.limits.command_timeout;
        let mut output = String::new();
        let mut dropped = 0_usize;
        let mut readers_done = false;
        let mut exited: Option<(std::process::ExitStatus, Instant)> = None;
        loop {
            if readers_done {
                std::thread::sleep(Duration::from_millis(10));
            } else {
                match chunks.recv_timeout(Duration::from_millis(20)) {
                    Ok((stream, bytes)) => {
                        let chunk = String::from_utf8_lossy(&bytes).into_owned();
                        let room = self.limits.max_output_bytes.saturating_sub(output.len());
                        if chunk.len() <= room {
                            output.push_str(&chunk);
                        } else {
                            dropped += chunk.len();
                        }
                        ctx.events.emit(EventKind::ToolOutput {
                            call_id: call_id.to_string(),
                            stream: stream.to_string(),
                            chunk,
                        });
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => readers_done = true,
                }
            }
            if exited.is_none() {
                match child.try_wait() {
                    Ok(Some(status)) => exited = Some((status, Instant::now())),
                    Ok(None) => {}
                    Err(error) => {
                        kill_tree(&mut child);
                        return ToolResult::error(call_id, format!("wait failed: {error}"));
                    }
                }
            }
            if let Some((status, at)) = exited {
                // A background grandchild can keep the pipes open; don't wait
                // on it forever once the command itself has exited.
                if readers_done || at.elapsed() > Duration::from_millis(500) {
                    if dropped > 0 {
                        let _ = write!(output, "\n… {dropped} bytes of output dropped");
                    }
                    let code = status.code();
                    let result = if status.success() {
                        ToolResult::ok(call_id, output)
                    } else {
                        ToolResult::error(
                            call_id,
                            format!(
                                "exit status {}\n{output}",
                                code.map_or("signal".into(), |c| c.to_string())
                            ),
                        )
                    };
                    return result.with_exit_code(code);
                }
                continue;
            }
            if ctx.is_cancelled() {
                kill_tree(&mut child);
                return ToolResult::error(call_id, format!("cancelled\n{output}"));
            }
            if Instant::now() >= deadline {
                kill_tree(&mut child);
                return ToolResult::error(
                    call_id,
                    format!(
                        "timed out after {} s\n{output}",
                        self.limits.command_timeout.as_secs()
                    ),
                );
            }
        }
    }

    fn http_fetch(&self, call_id: &str, url: &str) -> ToolResult {
        let Some(http) = &self.http else {
            return ToolResult::error(call_id, "http_fetch is not configured");
        };
        match http.fetch(url) {
            Ok(page) => {
                let mut output = format!("HTTP {}", page.status);
                if let Some(kind) = &page.content_type {
                    let _ = write!(output, " ({kind})");
                }
                output.push('\n');
                output.push_str(&page.body);
                if page.truncated {
                    output.push_str("\n… body truncated");
                }
                if (200..400).contains(&page.status) {
                    ToolResult::ok(call_id, output)
                } else {
                    ToolResult::error(call_id, output)
                }
            }
            Err(error) => ToolResult::error(call_id, error),
        }
    }
}

impl ToolExecutor for LocalTools {
    fn execute(&mut self, call_id: &str, request: &ToolRequest) -> ToolResult {
        let events = Emitter::new(TaskId::from_seed("detached"));
        let never = || false;
        let ctx = RunContext {
            events: &events,
            cancelled: &never,
        };
        self.execute_in(&ctx, call_id, request)
    }

    fn execute_in(
        &mut self,
        ctx: &RunContext<'_>,
        call_id: &str,
        request: &ToolRequest,
    ) -> ToolResult {
        match request {
            ToolRequest::Read { path } => self.read_file(call_id, path),
            ToolRequest::ListDir { path } => self.list_dir(call_id, path),
            ToolRequest::Search { pattern, path } => self.search(ctx, call_id, pattern, path),
            ToolRequest::Write { path, contents } => Self::write_file(call_id, path, contents),
            ToolRequest::ApplyPatch { path, patch } => self.apply_patch(call_id, path, patch),
            ToolRequest::Edit {
                path,
                old_string,
                new_string,
                replace_all,
            } => self.edit(call_id, path, old_string, new_string, *replace_all),
            ToolRequest::Exec { command, cwd } => self.run_command(ctx, call_id, command, cwd),
            ToolRequest::Network { url } => self.http_fetch(call_id, url),
            ToolRequest::AskUser { .. } => ToolResult::error(
                call_id,
                "ask_user is not available in M1; ask in your final message instead",
            ),
            ToolRequest::Learn(learn) => ToolResult::error(
                call_id,
                format!("tool `{}` needs the learning tools executor", learn.tool),
            ),
        }
    }
}

/// Read a UTF-8 file for editing: `Ok(None)` when it does not exist. Same
/// directory and size checks as `read_file`.
fn read_text(path: &Path, limit: u64) -> Result<Option<String>, ToolResult> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {
            return Err(ToolResult::error(
                "",
                format!("{} is a directory", path.display()),
            ))
        }
        Ok(meta) if meta.len() > limit => {
            return Err(ToolResult::error(
                "",
                format!(
                    "{} is {} bytes; the edit limit is {limit} bytes",
                    path.display(),
                    meta.len()
                ),
            ))
        }
        _ => {}
    }
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| ToolResult::error("", format!("{} is not UTF-8 text", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("", path, &error)),
    }
}

trait ForCall {
    fn for_call(self, call_id: &str) -> ToolResult;
}

impl ForCall for ToolResult {
    fn for_call(mut self, call_id: &str) -> ToolResult {
        call_id.clone_into(&mut self.call_id);
        self
    }
}

fn io_error(call_id: &str, path: &Path, error: &std::io::Error) -> ToolResult {
    ToolResult::error(call_id, format!("{}: {error}", path.display()))
}

/// Write via a sibling temp file + rename so readers never see a torn file.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy().into_owned());
    let temp: PathBuf = path.with_file_name(format!(".{name}.next-agent-tmp"));
    fs::write(&temp, contents)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

fn pump(
    stream: &'static str,
    mut reader: impl Read + Send + 'static,
    sender: mpsc::Sender<(&'static str, Vec<u8>)>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if sender.send((stream, buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

#[cfg(unix)]
fn shell(command: &str) -> Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    // Own process group so cancel can kill the whole tree.
    cmd.process_group(0);
    cmd
}

#[cfg(windows)]
fn shell(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

/// Kill the command and everything it spawned.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        // `unsafe` is forbidden in this workspace, so signal the process
        // group through the `kill` utility instead of libc.
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{}", child.id()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Collector;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("na-tools-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn run(request: &ToolRequest) -> ToolResult {
        LocalTools::new().execute("c1", request)
    }

    #[test]
    fn read_list_write_round_trip() {
        let dir = temp_dir("rw");
        let file = dir.join("nested/a.txt");
        let wrote = run(&ToolRequest::Write {
            path: file.clone(),
            contents: "hello\n".into(),
        });
        assert!(wrote.ok, "{}", wrote.output);
        assert_eq!(
            run(&ToolRequest::Read { path: file.clone() }).output,
            "hello\n"
        );
        let listing = run(&ToolRequest::ListDir { path: dir.clone() });
        assert_eq!(listing.output, "nested/");
        assert!(
            !run(&ToolRequest::Read {
                path: dir.join("missing")
            })
            .ok
        );
        assert!(!run(&ToolRequest::Read { path: dir.clone() }).ok);
    }

    #[test]
    fn read_rejects_oversized_and_binary_files() {
        let dir = temp_dir("big");
        let big = dir.join("big.txt");
        fs::write(&big, "x".repeat(20)).expect("write");
        let mut tools = LocalTools::new().with_limits(ToolLimits {
            max_read_bytes: 10,
            ..ToolLimits::default()
        });
        assert!(!tools.execute("c", &ToolRequest::Read { path: big }).ok);
        let bin = dir.join("bin");
        fs::write(&bin, [0xff_u8, 0xfe, 0x00]).expect("write");
        assert!(run(&ToolRequest::Read { path: bin })
            .output
            .contains("not UTF-8"));
    }

    #[test]
    fn search_respects_gitignore_and_reports_line_numbers() {
        let dir = temp_dir("search");
        fs::create_dir_all(dir.join(".git")).expect("git dir");
        fs::write(dir.join(".gitignore"), "ignored.txt\n").expect("ignore");
        fs::write(dir.join("a.rs"), "fn main() {}\n// TODO: fix\n").expect("a");
        fs::write(dir.join("ignored.txt"), "TODO: hidden\n").expect("ignored");
        let result = run(&ToolRequest::Search {
            pattern: "TODO".into(),
            path: dir.clone(),
        });
        assert!(result.ok);
        assert_eq!(result.output, "a.rs:2:// TODO: fix");
        let none = run(&ToolRequest::Search {
            pattern: "nothing-here".into(),
            path: dir.clone(),
        });
        assert_eq!(none.output, "no matches");
        assert!(
            !run(&ToolRequest::Search {
                pattern: "(".into(),
                path: dir
            })
            .ok
        );
    }

    #[test]
    fn search_stops_at_match_limit() {
        let dir = temp_dir("limit");
        fs::write(dir.join("a.txt"), "x\n".repeat(10)).expect("a");
        let mut tools = LocalTools::new().with_limits(ToolLimits {
            max_search_matches: 3,
            ..ToolLimits::default()
        });
        let result = tools.execute(
            "c",
            &ToolRequest::Search {
                pattern: "x".into(),
                path: dir,
            },
        );
        assert_eq!(result.output.lines().count(), 4);
        assert!(result.output.ends_with("stopped after 3 matches"));
    }

    #[test]
    fn edit_preserves_bom_and_crlf_on_disk() {
        let dir = temp_dir("edit");
        let file = dir.join("a.txt");
        fs::write(&file, "\u{feff}one\r\ntwo\r\n").expect("write");
        let edit = |old: &str, new: &str, all: bool| {
            run(&ToolRequest::Edit {
                path: file.clone(),
                old_string: old.into(),
                new_string: new.into(),
                replace_all: all,
            })
        };
        let result = edit("two\n", "TWO\nthree\n", false);
        assert!(result.ok, "{}", result.output);
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "\u{feff}one\r\nTWO\r\nthree\r\n"
        );
        let miss = edit("nope", "x", false);
        assert!(!miss.ok);
        assert!(miss.output.contains("Could not find old_string"));
        assert_eq!(miss.call_id, "c1");
        let missing = run(&ToolRequest::Edit {
            path: dir.join("missing.txt"),
            old_string: "a".into(),
            new_string: "b".into(),
            replace_all: false,
        });
        assert!(missing.output.contains("file not found"));
    }

    #[test]
    fn apply_patch_keeps_bom_and_crlf() {
        let dir = temp_dir("patch-crlf");
        let file = dir.join("a.txt");
        fs::write(&file, "\u{feff}one\r\ntwo\r\n").expect("write");
        let result = run(&ToolRequest::ApplyPatch {
            path: file.clone(),
            patch: "@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n".into(),
        });
        assert!(result.ok, "{}", result.output);
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "\u{feff}one\r\nTWO\r\n"
        );
    }

    #[test]
    fn apply_patch_edits_and_creates_files() {
        let dir = temp_dir("patch");
        let file = dir.join("a.txt");
        fs::write(&file, "one\ntwo\nthree\n").expect("write");
        let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let result = run(&ToolRequest::ApplyPatch {
            path: file.clone(),
            patch: patch.into(),
        });
        assert!(result.ok, "{}", result.output);
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "one\nTWO\nthree\n"
        );

        // Same patch no longer applies.
        let again = run(&ToolRequest::ApplyPatch {
            path: file.clone(),
            patch: patch.into(),
        });
        assert!(!again.ok);
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "one\nTWO\nthree\n"
        );

        let created = dir.join("new.txt");
        let result = run(&ToolRequest::ApplyPatch {
            path: created.clone(),
            patch: "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\n".into(),
        });
        assert!(result.ok, "{}", result.output);
        assert_eq!(fs::read_to_string(created).expect("read"), "hello\nworld\n");
        assert!(
            !run(&ToolRequest::ApplyPatch {
                path: file,
                patch: "garbage".into()
            })
            .ok
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_command_streams_output_and_reports_exit_code() {
        let dir = temp_dir("cmd");
        let collector = Collector::new();
        let events = Emitter::new(TaskId::from_seed("cmd")).with_sink(collector.clone());
        let never = || false;
        let ctx = RunContext {
            events: &events,
            cancelled: &never,
        };
        let mut tools = LocalTools::new();
        let ok = tools.execute_in(
            &ctx,
            "c1",
            &ToolRequest::Exec {
                command: "echo out; echo err 1>&2".into(),
                cwd: dir.clone(),
            },
        );
        assert!(ok.ok);
        assert_eq!(ok.exit_code, Some(0));
        let streams: Vec<String> = collector
            .events()
            .iter()
            .map(|e| e.data()["stream"].as_str().unwrap_or_default().to_string())
            .collect();
        assert!(streams.contains(&"stdout".to_string()));
        assert!(streams.contains(&"stderr".to_string()));

        let failed = tools.execute_in(
            &ctx,
            "c2",
            &ToolRequest::Exec {
                command: "exit 3".into(),
                cwd: dir,
            },
        );
        assert!(!failed.ok);
        assert_eq!(failed.exit_code, Some(3));
    }

    #[cfg(unix)]
    #[test]
    fn run_command_rejects_unresolvable_cwd() {
        let dir = temp_dir("cwd");
        let missing = dir.join("missing");
        let result = LocalTools::new().execute(
            "c",
            &ToolRequest::Exec {
                command: "pwd".into(),
                cwd: missing,
            },
        );
        assert!(!result.ok);
        assert!(result.output.contains("could not resolve exec cwd"));
    }

    #[cfg(unix)]
    #[test]
    fn run_command_times_out_and_kills() {
        let dir = temp_dir("timeout");
        let mut tools = LocalTools::new().with_limits(ToolLimits {
            command_timeout: Duration::from_millis(200),
            ..ToolLimits::default()
        });
        let started = Instant::now();
        let result = tools.execute(
            "c",
            &ToolRequest::Exec {
                command: "sleep 30".into(),
                cwd: dir,
            },
        );
        assert!(!result.ok);
        assert!(result.output.starts_with("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    struct StaticPage(u16);
    impl HttpFetch for StaticPage {
        fn fetch(&self, url: &str) -> Result<FetchedPage, String> {
            Ok(FetchedPage {
                status: self.0,
                content_type: Some("text/plain".into()),
                body: format!("body of {url}"),
                truncated: false,
            })
        }
    }

    #[test]
    fn http_fetch_uses_the_injected_client() {
        let request = ToolRequest::Network {
            url: "https://docs.rs/x".into(),
        };
        assert!(!run(&request).ok, "unconfigured fetch must fail");
        let mut tools = LocalTools::new().with_http(Arc::new(StaticPage(200)));
        let page = tools.execute("c", &request);
        assert!(page.ok);
        assert_eq!(
            page.output,
            "HTTP 200 (text/plain)\nbody of https://docs.rs/x"
        );
        let mut tools = LocalTools::new().with_http(Arc::new(StaticPage(404)));
        assert!(!tools.execute("c", &request).ok);
    }

    #[test]
    fn ask_user_is_refused() {
        assert!(
            !run(&ToolRequest::AskUser {
                question: "?".into()
            })
            .ok
        );
    }
}
