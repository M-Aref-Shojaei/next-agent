//! Design boards: Excalidraw scenes stored in the vault as `boards/<name>.excalidraw`.
//!
//! Everything here works on a vault path so it is testable without a window.
//! Writes are atomic (fsynced unique temp file + rename) and guarded by a
//! revision token so a save never silently overwrites a file that changed on
//! disk or resurrects one that was deleted. Creating, renaming and exporting
//! never replace an existing file. Deleting moves a board to
//! `<vault>/.trash/boards/`, never removes it. Symlinked `boards/`, `assets/`
//! or `.trash/` directories and non-regular board files are refused, so a link
//! planted in the vault cannot redirect reads or writes outside it.

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

const BOARDS_DIR: &[&str] = &["boards"];
const TRASH_DIR: &[&str] = &[".trash", "boards"];
const ASSETS_DIR: &[&str] = &["assets"];
const EXT: &str = "excalidraw";
const MAX_NAME: usize = 100;
const MAX_BOARD_BYTES: usize = 64 * 1024 * 1024;

/// One board in the list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMeta {
    pub name: String,
    pub modified_ms: u64,
    pub size: u64,
}

/// A board's JSON text plus the revision it was read at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardDoc {
    pub name: String,
    pub content: String,
    pub revision: String,
}

/// A board reduced to what the Design Studio needs: live elements and files.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoardSketch {
    pub name: String,
    pub elements: Vec<Value>,
    pub files: Value,
}

/// Names Windows reserves as devices, with or without an extension.
fn is_reserved(stem: &str) -> bool {
    let up = stem.trim_end().to_uppercase();
    if matches!(
        up.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    ["COM", "LPT"].iter().any(|p| {
        up.strip_prefix(p).is_some_and(|d| {
            matches!(
                d,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

/// Validate a board or asset base name: no separators, no leading/trailing
/// dots, no Windows reserved device names (also when followed by a dot part).
///
/// # Errors
/// Returns a message for empty, too long, hidden, reserved or path-like names.
pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("board name is empty".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("board name is longer than {MAX_NAME} characters"));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err("board name must not start or end with a dot".into());
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return Err("board name contains characters that are not allowed".into());
    }
    if is_reserved(name.split('.').next().unwrap_or(name)) {
        return Err("board name is reserved by the operating system".into());
    }
    Ok(name.to_string())
}

/// Resolve `vault/<parts…>`, refusing any component that is a symlink or not a
/// directory. With `create`, missing components are created. `Ok(None)` means
/// it does not exist (only when not creating).
fn real_dir(vault: &Path, parts: &[&str], create: bool) -> Result<Option<PathBuf>, String> {
    let mut dir = vault.to_path_buf();
    for part in parts {
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!(
                    "\"{part}\" in the vault is a symlink; refusing to use it"
                ));
            }
            Ok(m) if !m.is_dir() => return Err(format!("\"{part}\" in the vault is not a folder")),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if !create {
                    return Ok(None);
                }
                fs::create_dir(&dir).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(Some(dir))
}

fn board_path(vault: &Path, name: &str, create_dir: bool) -> Result<PathBuf, String> {
    let dir = real_dir(vault, BOARDS_DIR, create_dir)?.unwrap_or_else(|| vault.join("boards"));
    Ok(dir.join(format!("{}.{EXT}", validate_name(name)?)))
}

/// Metadata of a regular file (not following symlinks), or `None` if absent.
fn regular_meta(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(m)),
        Ok(_) => Err("not a regular file; refusing to use it".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn revision_of(meta: &fs::Metadata) -> String {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("{nanos}-{}", meta.len())
}

fn validate_scene(content: &str) -> Result<(), String> {
    if content.len() > MAX_BOARD_BYTES {
        return Err("board is too large".into());
    }
    let value: Value =
        serde_json::from_str(content).map_err(|e| format!("not valid board JSON: {e}"))?;
    if value.get("type").and_then(Value::as_str) != Some("excalidraw") {
        return Err("not an Excalidraw scene (missing type \"excalidraw\")".into());
    }
    Ok(())
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bytes` to a fresh uniquely named temp file next to `path`, fsynced.
fn write_tmp(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{file}.{}.{n}.tmp", std::process::id()));
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
    match result {
        Ok(()) => Ok(tmp),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Best effort: persist the rename itself.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Replace `path` atomically (fsynced temp file + rename).
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = write_tmp(path, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })?;
    sync_dir(path);
    Ok(())
}

/// Move `src` to `dst` without ever replacing `dst`. A hard link is atomic and
/// fails with `AlreadyExists`; where links are unsupported, fall back to a
/// check followed by a rename.
fn no_replace_rename(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::hard_link(src, dst) {
        Ok(()) => fs::remove_file(src),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        Err(_) => {
            if fs::symlink_metadata(dst).is_ok() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            fs::rename(src, dst)
        }
    }
}

/// Create `path` with `bytes`, failing with `AlreadyExists` instead of replacing.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = write_tmp(path, bytes)?;
    let result = no_replace_rename(&tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    } else {
        sync_dir(path);
    }
    result
}

/// The scene a new board starts with.
#[must_use]
pub fn empty_scene() -> String {
    r##"{"type":"excalidraw","version":2,"source":"next-agent","elements":[],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##.to_string()
}

/// List boards, newest first.
///
/// # Errors
/// Returns a message on I/O failure or if `boards/` is a symlink.
pub fn list(vault: &Path) -> Result<Vec<BoardMeta>, String> {
    let Some(dir) = real_dir(vault, BOARDS_DIR, false)? else {
        return Ok(Vec::new());
    };
    let mut boards = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.starts_with('.') {
            continue;
        }
        // `DirEntry::metadata` does not follow symlinks.
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() {
            continue;
        }
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        boards.push(BoardMeta {
            name: stem.to_string(),
            modified_ms,
            size: meta.len(),
        });
    }
    boards.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(boards)
}

/// Create a board. Fails if one with that name exists.
///
/// # Errors
/// Returns a message if the name is invalid, taken, or the scene is invalid.
pub fn create(vault: &Path, name: &str, scene: Option<&str>) -> Result<BoardDoc, String> {
    let path = board_path(vault, name, true)?;
    let content = scene.map_or_else(empty_scene, str::to_string);
    validate_scene(&content)?;
    write_new(&path, content.as_bytes()).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            format!("a board named \"{}\" already exists", name.trim())
        } else {
            e.to_string()
        }
    })?;
    open(vault, name)
}

/// Read a board.
///
/// # Errors
/// Returns a message if the board does not exist, is not a regular file, is
/// too large, or cannot be read.
pub fn open(vault: &Path, name: &str) -> Result<BoardDoc, String> {
    let path = board_path(vault, name, false)?;
    let meta =
        regular_meta(&path)?.ok_or_else(|| format!("board \"{}\" not found", name.trim()))?;
    if meta.len() > MAX_BOARD_BYTES as u64 {
        return Err("board is too large".into());
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(BoardDoc {
        name: validate_name(name)?,
        content,
        revision: revision_of(&meta),
    })
}

/// Save a board atomically. When `base_revision` is given and the file on disk
/// has a different revision, or is gone, the save is refused with a
/// `conflict:` error and nothing is written. Returns the new revision.
///
/// # Errors
/// Returns a message on invalid input, conflict, or I/O failure.
pub fn save(
    vault: &Path,
    name: &str,
    content: &str,
    base_revision: Option<&str>,
) -> Result<String, String> {
    validate_scene(content)?;
    let path = board_path(vault, name, true)?;
    let current = regular_meta(&path)?;
    if let Some(base) = base_revision {
        match &current {
            None => {
                return Err(format!(
                    "conflict: deleted: board \"{}\" no longer exists on disk",
                    name.trim()
                ));
            }
            Some(meta) if revision_of(meta) != base => {
                return Err(format!(
                    "conflict: board \"{}\" changed on disk since it was opened",
                    name.trim()
                ));
            }
            Some(_) => {}
        }
    }
    atomic_write(&path, content.as_bytes())?;
    let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    Ok(revision_of(&meta))
}

/// Rename a board. Fails if the target exists. A change of letter case only is
/// allowed even on case-insensitive filesystems.
///
/// # Errors
/// Returns a message if the source is missing or the target exists.
pub fn rename(vault: &Path, from: &str, to: &str) -> Result<BoardMeta, String> {
    let src = board_path(vault, from, false)?;
    let dst = board_path(vault, to, false)?;
    if regular_meta(&src)?.is_none() {
        return Err(format!("board \"{}\" not found", from.trim()));
    }
    let exists = |e: &io::Error| e.kind() == io::ErrorKind::AlreadyExists;
    let taken = || format!("a board named \"{}\" already exists", to.trim());
    if from.trim().to_lowercase() == to.trim().to_lowercase() {
        // Same file on a case-insensitive filesystem: a plain rename is the
        // only way; on a case-sensitive one a distinct file must not be replaced.
        if from.trim() != to.trim() {
            let same_file = fs::canonicalize(&src).ok() == fs::canonicalize(&dst).ok();
            if regular_meta(&dst)?.is_some() && !same_file {
                return Err(taken());
            }
            fs::rename(&src, &dst).map_err(|e| e.to_string())?;
        }
    } else {
        no_replace_rename(&src, &dst)
            .map_err(|e| if exists(&e) { taken() } else { e.to_string() })?;
    }
    list(vault)?
        .into_iter()
        .find(|b| b.name == to.trim())
        .ok_or_else(|| "renamed board vanished".to_string())
}

/// Move a board to the vault trash (`.trash/boards/<name>-<stamp>.excalidraw`).
/// Returns the trash path relative to the vault.
///
/// # Errors
/// Returns a message if the board is missing or cannot be moved.
pub fn delete(vault: &Path, name: &str) -> Result<String, String> {
    let src = board_path(vault, name, false)?;
    if regular_meta(&src)?.is_none() {
        return Err(format!("board \"{}\" not found", name.trim()));
    }
    let trash = real_dir(vault, TRASH_DIR, true)?.ok_or("cannot create trash folder")?;
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let base = validate_name(name)?;
    let mut n = 0;
    loop {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let target = trash.join(format!("{base}-{stamp}{suffix}.{EXT}"));
        match no_replace_rename(&src, &target) {
            Ok(()) => {
                let relative = target.strip_prefix(vault).unwrap_or(&target);
                return Ok(relative.display().to_string());
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Live (non-deleted) elements and embedded files of a board, for the Design Studio.
///
/// # Errors
/// Returns a message if the board is missing or not valid JSON.
pub fn sketch(vault: &Path, name: &str) -> Result<BoardSketch, String> {
    let doc = open(vault, name)?;
    let value: Value = serde_json::from_str(&doc.content).map_err(|e| e.to_string())?;
    let elements = value
        .get("elements")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |all| {
            all.iter()
                .filter(|e| e.get("isDeleted").and_then(Value::as_bool) != Some(true))
                .cloned()
                .collect()
        });
    Ok(BoardSketch {
        name: doc.name,
        elements,
        files: value.get("files").cloned().unwrap_or(Value::Null),
    })
}

/// Save an exported image (`png` or `svg`) as `assets/<name>.<ext>`. `data` is
/// base64 for PNG and plain text for SVG. Existing files get a numeric suffix
/// so exports never overwrite earlier ones. Returns the vault-relative path.
///
/// # Errors
/// Returns a message for an invalid name, extension or payload.
pub fn export_asset(vault: &Path, name: &str, ext: &str, data: &str) -> Result<String, String> {
    let base = validate_name(name)?;
    let bytes = match ext {
        "png" => base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|e| format!("invalid PNG data: {e}"))?,
        "svg" => data.as_bytes().to_vec(),
        _ => return Err(format!("unsupported export type \"{ext}\"")),
    };
    if ext == "png" && !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err("data is not a PNG".into());
    }
    let dir = real_dir(vault, ASSETS_DIR, true)?.ok_or("cannot create assets folder")?;
    let mut n = 0;
    loop {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let target = dir.join(format!("{base}{suffix}.{ext}"));
        match write_new(&target, &bytes) {
            Ok(()) => {
                let relative = target.strip_prefix(vault).unwrap_or(&target);
                return Ok(relative.display().to_string());
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("Flow 1").is_ok());
        for bad in ["", "  ", "../x", "a/b", ".hidden", "a\\b", "x.", "a:b"] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(validate_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn create_open_list_round_trip() {
        let v = vault();
        assert_eq!(list(v.path()).expect("list").len(), 0);
        let doc = create(v.path(), "Sketch", None).expect("create");
        assert_eq!(doc.name, "Sketch");
        assert!(v.path().join("boards/Sketch.excalidraw").is_file());
        assert_eq!(open(v.path(), "Sketch").expect("open").content, doc.content);
        let boards = list(v.path()).expect("list");
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].name, "Sketch");
        assert!(create(v.path(), "Sketch", None).is_err(), "duplicate");
    }

    #[test]
    fn create_rejects_non_excalidraw_json() {
        let v = vault();
        assert!(create(v.path(), "a", Some("{\"type\":\"other\"}")).is_err());
        assert!(create(v.path(), "a", Some("nope")).is_err());
        assert!(!v.path().join("boards/a.excalidraw").exists());
    }

    #[test]
    fn save_is_atomic_and_leaves_no_temp_files() {
        let v = vault();
        let doc = create(v.path(), "a", None).expect("create");
        let next = empty_scene().replace("\"elements\":[]", "\"elements\":[{\"id\":\"x\"}]");
        let rev = save(v.path(), "a", &next, Some(&doc.revision)).expect("save");
        assert_ne!(rev, doc.revision);
        assert_eq!(open(v.path(), "a").expect("open").content, next);
        let leftovers: Vec<_> = fs::read_dir(v.path().join("boards"))
            .expect("dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        // An invalid payload never replaces the file.
        assert!(save(v.path(), "a", "garbage", None).is_err());
        assert_eq!(open(v.path(), "a").expect("open").content, next);
    }

    #[test]
    fn save_detects_changes_on_disk() {
        let v = vault();
        let doc = create(v.path(), "a", None).expect("create");
        // Someone else edits the file.
        let theirs = empty_scene().replace(
            "\"elements\":[]",
            "\"elements\":[{\"id\":\"t\"},{\"id\":\"u\"}]",
        );
        fs::write(v.path().join("boards/a.excalidraw"), &theirs).expect("write");
        let err = save(v.path(), "a", &empty_scene(), Some(&doc.revision)).expect_err("conflict");
        assert!(err.starts_with("conflict:"), "{err}");
        assert_eq!(open(v.path(), "a").expect("open").content, theirs);
        // Saving with the fresh revision (or none, to overwrite) works.
        let fresh = open(v.path(), "a").expect("open").revision;
        assert!(save(v.path(), "a", &empty_scene(), Some(&fresh)).is_ok());
        assert!(save(v.path(), "a", &theirs, None).is_ok());
    }

    #[test]
    fn rename_moves_and_refuses_overwrite() {
        let v = vault();
        create(v.path(), "a", None).expect("a");
        create(v.path(), "b", None).expect("b");
        assert!(rename(v.path(), "a", "b").is_err());
        assert!(rename(v.path(), "missing", "c").is_err());
        let meta = rename(v.path(), "a", "c").expect("rename");
        assert_eq!(meta.name, "c");
        assert!(!v.path().join("boards/a.excalidraw").exists());
        assert!(v.path().join("boards/c.excalidraw").is_file());
    }

    #[test]
    fn delete_moves_to_trash_and_keeps_content() {
        let v = vault();
        let doc = create(v.path(), "a", None).expect("create");
        let trashed = delete(v.path(), "a").expect("delete");
        assert!(trashed.starts_with(".trash/boards/a-"), "{trashed}");
        assert_eq!(list(v.path()).expect("list").len(), 0);
        assert_eq!(
            fs::read_to_string(v.path().join(&trashed)).expect("read"),
            doc.content
        );
        // Same name again does not collide in the trash.
        create(v.path(), "a", None).expect("again");
        let second = delete(v.path(), "a").expect("delete again");
        assert_ne!(trashed, second);
        assert!(delete(v.path(), "a").is_err());
    }

    #[test]
    fn sketch_drops_deleted_elements() {
        let v = vault();
        let scene = r#"{"type":"excalidraw","version":2,"elements":[{"id":"1","type":"rectangle"},{"id":"2","type":"text","isDeleted":true}],"appState":{},"files":{"f":{}}}"#;
        create(v.path(), "s", Some(scene)).expect("create");
        let s = sketch(v.path(), "s").expect("sketch");
        assert_eq!(s.elements.len(), 1);
        assert_eq!(s.elements[0]["id"], "1");
        assert!(s.files.get("f").is_some());
    }

    #[test]
    fn export_asset_writes_png_and_svg_without_overwriting() {
        let v = vault();
        let png = base64::engine::general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G', 1, 2]);
        let first = export_asset(v.path(), "flow", "png", &png).expect("png");
        let second = export_asset(v.path(), "flow", "png", &png).expect("png2");
        assert_eq!(first, "assets/flow.png");
        assert_eq!(second, "assets/flow-1.png");
        assert_eq!(
            export_asset(v.path(), "flow", "svg", "<svg/>").expect("svg"),
            "assets/flow.svg"
        );
        assert!(export_asset(v.path(), "x", "png", "bm90LXBuZw==").is_err());
        assert!(export_asset(v.path(), "x", "exe", "").is_err());
        assert!(export_asset(v.path(), "../x", "svg", "").is_err());
    }

    #[test]
    fn reserved_windows_names_are_rejected() {
        for bad in [
            "CON",
            "con",
            "Prn",
            "AUX",
            "nul",
            "COM1",
            "com9",
            "LPT1",
            "lpt9",
            "CON.txt",
            "aux.tar.gz",
            "NUL.x",
            "con .y",
            "COM¹",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
        for ok in [
            "COM10",
            "CONSOLE",
            "console.log",
            "LPT0",
            "my.con",
            "Flow CON",
        ] {
            assert!(validate_name(ok).is_ok(), "{ok:?} should be accepted");
        }
        let v = vault();
        assert!(create(v.path(), "NUL", None).is_err());
        assert!(export_asset(v.path(), "COM1", "svg", "<svg/>").is_err());
        assert!(export_asset(v.path(), "aux.v2", "svg", "<svg/>").is_err());
    }

    #[test]
    fn save_with_a_base_revision_never_resurrects_a_deleted_board() {
        let v = vault();
        let doc = create(v.path(), "a", None).expect("create");
        delete(v.path(), "a").expect("delete");
        let err = save(v.path(), "a", &empty_scene(), Some(&doc.revision)).expect_err("deleted");
        assert!(err.starts_with("conflict: deleted"), "{err}");
        assert!(!v.path().join("boards/a.excalidraw").exists());
        // An explicit overwrite (no base revision) still creates it.
        assert!(save(v.path(), "a", &empty_scene(), None).is_ok());
    }

    #[test]
    fn create_and_export_never_clobber() {
        let v = vault();
        let path = v.path().join("boards/x.excalidraw");
        fs::create_dir_all(v.path().join("boards")).expect("dir");
        write_new(&path, b"one").expect("first");
        let err = write_new(&path, b"two").expect_err("exists");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).expect("read"), b"one");
        let leftovers = fs::read_dir(v.path().join("boards"))
            .expect("dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        // Rename onto an existing board is refused and keeps both files.
        create(v.path(), "a", None).expect("a");
        create(
            v.path(),
            "b",
            Some(&empty_scene().replace("[]", "[{\"id\":\"b\"}]")),
        )
        .expect("b");
        assert!(rename(v.path(), "a", "b").is_err());
        assert!(open(v.path(), "b")
            .expect("b")
            .content
            .contains("\"id\":\"b\""));
    }

    #[test]
    fn temp_names_are_unique() {
        let v = vault();
        let p = v.path().join("f.txt");
        let a = write_tmp(&p, b"1").expect("a");
        let b = write_tmp(&p, b"2").expect("b");
        assert_ne!(a, b);
    }

    #[test]
    fn case_only_rename_works() {
        let v = vault();
        create(v.path(), "flow", None).expect("create");
        let meta = rename(v.path(), "flow", "Flow").expect("case rename");
        assert_eq!(meta.name, "Flow");
        assert_eq!(list(v.path()).expect("list").len(), 1);
        assert!(open(v.path(), "Flow").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_refused() {
        use std::os::unix::fs::symlink;
        let v = vault();
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("secret.excalidraw"), empty_scene()).expect("write");

        // A symlinked boards/ folder.
        symlink(outside.path(), v.path().join("boards")).expect("link");
        assert!(list(v.path()).is_err());
        assert!(create(v.path(), "x", None).is_err());
        assert!(open(v.path(), "secret").is_err());
        assert!(!outside.path().join("x.excalidraw").exists());
        fs::remove_file(v.path().join("boards")).expect("unlink");

        // A symlinked board file.
        fs::create_dir(v.path().join("boards")).expect("dir");
        symlink(
            outside.path().join("secret.excalidraw"),
            v.path().join("boards/s.excalidraw"),
        )
        .expect("link");
        assert!(open(v.path(), "s").is_err());
        assert!(save(v.path(), "s", &empty_scene(), None).is_err());
        assert!(delete(v.path(), "s").is_err());
        assert_eq!(list(v.path()).expect("list").len(), 0);

        // Symlinked assets/ and .trash/.
        symlink(outside.path(), v.path().join("assets")).expect("link");
        assert!(export_asset(v.path(), "f", "svg", "<svg/>").is_err());
        create(v.path(), "t", None).expect("t");
        fs::create_dir(v.path().join(".trash")).expect("dir");
        symlink(outside.path(), v.path().join(".trash/boards")).expect("link");
        assert!(delete(v.path(), "t").is_err());
        assert!(v.path().join("boards/t.excalidraw").is_file());
        assert_eq!(fs::read_dir(outside.path()).expect("dir").count(), 1);
    }

    #[test]
    fn oversized_boards_are_not_opened() {
        let v = vault();
        create(v.path(), "big", None).expect("create");
        let f = fs::OpenOptions::new()
            .write(true)
            .open(v.path().join("boards/big.excalidraw"))
            .expect("open");
        f.set_len(MAX_BOARD_BYTES as u64 + 1).expect("sparse");
        assert!(open(v.path(), "big").is_err());
    }
}
