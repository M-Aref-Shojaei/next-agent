// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/platform/{windows,linux}.rs, translated to macOS from NotchBuddy/Sources/App/{IslandWindowController,TerminalTarget,TerminalTarget+AppKit,UserIdentity,SessionLauncher,ClaudeHost}.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// macOS: Next Agent's translation of Coucou's platform layer. Coucou's Tauri
// app ships for Windows and Linux only (its Mac app is Swift), so this file
// gives the rest of the app the same set of names on a Mac, from what the
// Swift app does:
//   * the island window is Coucou's `IslandPanel` (IslandWindowController):
//     a non-activating NSPanel at `mainMenu + 3` that may cover the notch and
//     the menu bar; the webview moves into it (next_agent_macos_panel::adopt);
//   * like on Windows, a cursor poll decides click-through (the panel's
//     `ignoresMouseEvents`), the eyes and the drag;
//   * "Open terminal" brings the session's terminal app forward by its bundle
//     (TerminalTarget+AppKit: `openApplication(at:)`, here `open -a`);
//   * folders, links and the folder picker go through `open` and AppleScript's
//     `choose folder` (NSOpenPanel in SessionLauncher).
// Every AppKit or libc call goes through next_agent_macos_panel, Next Agent's
// only crate allowed `unsafe`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use next_agent_macos_panel as panel;
use tauri::{AppHandle, WebviewWindow};

use super::{home_dir, LocalTime};

/// File name of the Claude Code relay.
pub const HOOK_EXE: &str = "coucou-hook";

/// Environment variable holding the home directory.
pub const HOME_VAR: &str = "HOME";

/// Next Agent's app identifier: Coucou's files live in a folder of their own
/// under Next Agent's Application Support folder.
const APP_ID: &str = "dev.nextagent.desktop";

// ── Files ─────────────────────────────────────────────────────────────────────

/// ~/Library/Application Support/dev.nextagent.desktop/coucou — preferences
/// (the Swift app keeps its own under its bundle id the same way).
pub fn config_dir() -> PathBuf {
    home_dir()
        .join("Library/Application Support")
        .join(APP_ID)
        .join("coucou")
}

/// Same folder: coucou-hook, the inbox and the log. One folder, so the relay
/// sits at a stable path and the socket beside it stays short (sun_path is
/// 104 bytes on macOS, HookServer.swift).
pub fn local_dir() -> PathBuf {
    config_dir()
}

/// Where a saved image goes, best first; the caller takes the first that exists.
pub fn picture_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    vec![
        home.join("Pictures"),
        home.join("Downloads"),
        home.join("Desktop"),
        home,
    ]
}

/// Nothing to set before the webview starts on a Mac.
pub fn prepare_environment() {}

pub fn local_time() -> LocalTime {
    let (year, month, day, hour, minute, second) = panel::sys::local_time();
    LocalTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Creates `dir` and closes it to other users (HookServer.swift makes its
/// support directory 0700 the same way).
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// A real directory (not a symlink), owned by us, closed to group and others.
fn is_private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir)
        .map(|m| m.file_type().is_dir() && m.uid() == panel::sys::uid() && m.mode() & 0o077 == 0)
        .unwrap_or(false)
}

/// Where coucou-hook finds us: `coucou.sock` in the private config folder, as
/// the Swift app keeps `nb.sock` in its support directory. Must match
/// `socket_path()` in hook/src/mac.rs exactly.
pub fn relay_socket_path() -> Option<PathBuf> {
    let dir = config_dir();
    let _ = ensure_private_dir(&dir);
    is_private_dir(&dir).then(|| dir.join("coucou.sock"))
}

/// The user id, for the relay socket's peer check (pipe.rs).
pub fn current_uid() -> u32 {
    panel::sys::uid()
}

// ── Who we are ────────────────────────────────────────────────────────────────

/// `NSFullUserName()`, as UserIdentity.swift reads it.
pub fn user_full_name() -> Option<String> {
    panel::sys::full_user_name()
}

// ── Processes ─────────────────────────────────────────────────────────────────

/// Outlook (the Mail pill) is a Windows program: nothing to look for here.
pub fn process_running(_exe: &str) -> bool {
    false
}

/// Nothing to hide or undo for a child process on a Mac.
pub fn no_console(cmd: &mut Command) -> &mut Command {
    cmd
}

pub fn open_url(url: &str) {
    let _ = Command::new("/usr/bin/open").arg(url).spawn();
}

// Next Agent: removed WhatsApp/Spotify (owner request): handles_scheme (WhatsApp's app check).

pub fn reveal_folder(path: &str) {
    let _ = Command::new("/usr/bin/open").arg(path).spawn();
}

/// "Choose a folder…" for a new Claude Code session: the system's folder
/// picker (SessionLauncher.swift's NSOpenPanel), through AppleScript's
/// `choose folder`. The title and start folder are passed as arguments, never
/// spliced into the script. None when it was cancelled.
pub fn pick_folder(_app: &AppHandle, start: Option<&str>, title: &str) -> Option<String> {
    let start = start
        .map(str::to_string)
        .unwrap_or_else(|| home_dir().to_string_lossy().to_string());
    let out = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "POSIX path of (choose folder with prompt (item 1 of argv) default location (POSIX file (item 2 of argv)))",
            "-e",
            "end run",
            title,
            &start,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches(['\n', '\r'])
        .to_string();
    let path = if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    };
    (!path.is_empty()).then_some(path)
}

/// Our own `which`: the first executable file named `stem` on $PATH.
pub fn find_on_path(stem: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH")?;
    std::env::split_paths(&dirs)
        .map(|dir| dir.join(stem))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The Claude desktop app (`com.anthropic.claudefordesktop`, ClaudeHost.swift).
pub fn open_claude_desktop() -> bool {
    Command::new("/usr/bin/open")
        .args(["-b", "com.anthropic.claudefordesktop"])
        .status()
        .is_ok_and(|s| s.success())
}

/// Where a CLI such as `codex` or `claude` may be, best first: $PATH, then the
/// usual per-user and Homebrew folders, which an app started from the Dock
/// leaves out of $PATH, and nvm with its newest Node first.
pub fn cli_candidates(name: &str) -> Vec<PathBuf> {
    let home = home_dir();
    let mut out: Vec<PathBuf> = find_on_path(name).into_iter().collect();
    for dir in [
        ".local/bin",
        ".claude/local",
        ".npm-global/bin",
        ".volta/bin",
        ".bun/bin",
        "Library/pnpm",
    ] {
        out.push(home.join(dir).join(name));
    }
    out.push(PathBuf::from("/opt/homebrew/bin").join(name));
    out.push(PathBuf::from("/usr/local/bin").join(name));
    let nvm = home.join(".nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(&nvm) {
        let mut versions: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        versions.sort_by(|a, b| crate::codex_plan::compare_versions(b, a));
        out.extend(versions.iter().map(|v| nvm.join(v).join("bin").join(name)));
    }
    out.retain(|p| is_executable(p));
    out
}

// ── Cursor ────────────────────────────────────────────────────────────────────

/// As on Windows (and in IslandWindowController.swift), a cursor poll reads
/// the pointer and flips click-through from it.
pub const CURSOR_POLL: bool = true;

/// The pointer in the same space as Tauri's monitor and window positions on
/// a Mac: points scaled by the backing scale of the display under it.
pub fn cursor_physical() -> Option<(f64, f64)> {
    let (x, y) = panel::sys::cursor_points()?;
    let scale = panel::sys::display_scale_at(x, y);
    Some((x * scale, y * scale))
}

/// The pointer relative to the island panel, in points (y down), and the
/// panel's size: what island.rs's poll needs on a Mac, where the island lives
/// in the panel instead of the Tauri window.
pub fn cursor_in_island() -> Option<((f64, f64), (f64, f64))> {
    let size = (*ISLAND_SIZE.lock().unwrap())?;
    Some((panel::cursor_in_panel()?, size))
}

pub fn left_button_down() -> bool {
    panel::sys::left_button_down()
}

// ── Island window ─────────────────────────────────────────────────────────────

/// The island panel's current size (points), once it is placed.
static ISLAND_SIZE: Mutex<Option<(f64, f64)>> = Mutex::new(None);

fn is_island(win: &WebviewWindow) -> bool {
    win.label() == crate::island::WINDOW_LABEL
}

/// AppKit delivers drags to the panel whatever `ignoresMouseEvents` says.
pub fn unblock_webview_drops(_app: &AppHandle) {}

/// IslandWindowController.setupPanel: the island's webview moves into a
/// non-activating NSPanel at `mainMenu + 3`, on every Space, beside full-screen
/// apps and out of window cycling. Other windows stay ordinary Tauri windows,
/// kept above the others and out of the Dock's window list.
pub fn make_non_activating(win: &WebviewWindow) {
    if !is_island(win) {
        let _ = win.set_always_on_top(true);
        let _ = win.set_visible_on_all_workspaces(true);
        return;
    }
    let Ok(ns_window) = win.ns_window() else {
        return;
    };
    let size = (crate::island::PANEL_W, crate::island::PANEL_H);
    match panel::adopt(ns_window, size) {
        Ok(()) => {
            *ISLAND_SIZE.lock().unwrap() = Some(size);
            // The panel starts click-through; island.rs's poll starts from
            // "takes the mouse", as a new Tauri window does. Match it, or the
            // first pass over the island would never be let through.
            let _ = panel::set_ignores_mouse(false);
        }
        Err(err) => crate::log::line(format!("island panel: {err}")),
    }
}

/// Places the island panel: the top-left corner (global logical points, y
/// down) and the size, as island.rs computed them for the chosen display.
pub fn place_island(app: &AppHandle, x: f64, y: f64, w: f64, h: f64) {
    *ISLAND_SIZE.lock().unwrap() = Some((w, h));
    let _ = app.run_on_main_thread(move || {
        if panel::place_frame(x, y, w, h) {
            let _ = panel::order_front();
        }
    });
}

/// A text field in the island has focus: the panel takes the keyboard (the
/// Swift app makes its panel key for its text fields), then gives it back.
pub fn set_activating(win: &WebviewWindow, activating: bool) {
    if !is_island(win) {
        return;
    }
    let _ = win.run_on_main_thread(move || {
        let _ = panel::set_key(activating);
    });
}

/// Click-through: the island panel's `ignoresMouseEvents`, or the window's
/// own flag for the others. AppKit calls run on the main thread.
pub fn set_click_through(win: &WebviewWindow, ignore: bool) {
    if !is_island(win) {
        let _ = win.set_ignore_cursor_events(ignore);
        return;
    }
    let _ = win.run_on_main_thread(move || {
        let _ = panel::set_ignores_mouse(ignore);
    });
}

/// Click-through here is the poll's toggle, not a region.
pub fn set_input_region(_win: &WebviewWindow, _rect: Option<(f64, f64, f64, f64)>) {}

/// The cursor poll already parks itself with the island (island.rs).
pub fn set_pointer_watch(_active: bool) {}

// ── Session windows ("Open terminal") ─────────────────────────────────────────
//
// TerminalTarget+AppKit.swift brings the app the session runs in forward by
// its bundle. Here the relay's ancestors lead to it, as on Windows and Linux:
// the nearest ancestor that is an app with a bundle (Terminal, iTerm, Ghostty,
// VS Code, Cursor…) is the one.

/// `ps` once for the whole table, so no process is read twice.
fn process_table() -> std::collections::HashMap<u32, crate::session_window::Proc> {
    let mut procs = std::collections::HashMap::new();
    let Ok(out) = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,comm="])
        .output()
    else {
        return procs;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split_whitespace();
        let (Some(pid), Some(parent)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(pid), Ok(parent)) = (pid.parse::<u32>(), parent.parse::<u32>()) else {
            continue;
        };
        let command: Vec<&str> = parts.collect();
        let command = command.join(" ");
        let exe = Path::new(&command)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(command);
        procs.insert(pid, crate::session_window::Proc { parent, exe });
    }
    procs
}

/// The ancestors of a process, nearest first, below the top of the tree.
pub fn process_ancestors(pid: u32) -> Vec<u32> {
    crate::session_window::ancestors(&process_table(), pid)
}

/// The ancestors that are apps with a bundle, nearest first.
pub fn window_owners(ancestors: &[u32]) -> Vec<u32> {
    ancestors
        .iter()
        .copied()
        .filter(|pid| {
            i32::try_from(*pid)
                .ok()
                .and_then(panel::sys::app_bundle_path)
                .is_some()
        })
        .collect()
}

/// Brings the session's app forward: the nearest remembered owner still
/// running, opened by its bundle (TerminalTarget+AppKit.swift). The Swift app
/// brings the app forward, not one of its windows, so `folder` is not needed.
pub fn focus_session_window(owners: &[u32], _folder: &str) -> bool {
    owners.iter().any(|pid| {
        let Some(bundle) = i32::try_from(*pid)
            .ok()
            .and_then(panel::sys::app_bundle_path)
        else {
            return false;
        };
        Command::new("/usr/bin/open")
            .arg("-a")
            .arg(bundle)
            .status()
            .is_ok_and(|s| s.success())
    })
}

// ── Global shortcuts ──────────────────────────────────────────────────────────

/// The global-shortcut plugin works on macOS (HotKeyCenter.swift registers
/// Carbon hot keys the same way).
pub fn global_shortcuts_blocked() -> Option<&'static str> {
    None
}

/// Ctrl+Alt is not AltGr on a Mac: it types nothing of its own.
pub fn ctrl_alt_types(_vk: u16, _shift: bool) -> Option<String> {
    None
}

// ── Desktop Mochi window ──────────────────────────────────────────────────────

/// A Mac places a window wherever it is asked, and the cursor poll is there,
/// as on Windows (DesktopMochi.swift).
pub fn desktop_mode() -> super::DesktopMode {
    super::DesktopMode::Poll
}

pub fn prepare_desktop_window(win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    make_non_activating(win);
    true
}

/// The cursor poll toggles click-through from the body hit test instead.
pub fn set_desktop_shape(_win: &WebviewWindow, _shape: super::MouseShape) {}

/// Layer-shell only (Linux).
pub fn set_layer_margins(_win: &WebviewWindow, _x: f64, _y: f64) {}

/// Layer-shell only (Linux).
pub fn set_layer_overlay(_win: &WebviewWindow, _on: bool) {}

/// Layer-shell only (Linux).
pub fn layer_display(_island: &WebviewWindow, _mochi: &WebviewWindow) -> Option<(f64, f64)> {
    None
}

// ── Flow dictation's pill ─────────────────────────────────────────────────────
// As on Windows: placed with set_position, a cursor poll decides
// click-through (flow/pill.rs).

pub fn prepare_pill_window(win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    make_non_activating(win);
    let _ = win.set_ignore_cursor_events(true);
    true
}

pub fn set_pill_edge(_win: &WebviewWindow, _edge: &str) {}

pub fn set_pill_input(_win: &WebviewWindow, _rects: Vec<(f64, f64, f64, f64)>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coucou_files_live_under_next_agents_support_folder() {
        let dir = config_dir();
        assert!(dir.ends_with("Library/Application Support/dev.nextagent.desktop/coucou"));
        assert_eq!(local_dir(), dir);
    }

    #[test]
    fn the_relay_socket_path_fits_in_sun_path() {
        // 104 bytes with the NUL on macOS (HookServer.swift). A long account
        // name, not $HOME: another test points $HOME at a temp folder.
        let path = Path::new("/Users/firstname.lastname-long")
            .join("Library/Application Support")
            .join(APP_ID)
            .join("coucou/coucou.sock");
        assert!(path.as_os_str().len() < 104, "{}", path.display());
    }

    #[test]
    fn the_process_table_finds_this_process_and_its_parent() {
        let me = std::process::id();
        let table = process_table();
        let proc = table.get(&me).expect("this process is listed");
        assert_eq!(proc.parent, std::os::unix::process::parent_id());
    }
}
