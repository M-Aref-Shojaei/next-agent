//! Design boards: Excalidraw scenes stored in the vault as `boards/<name>.excalidraw`.
//!
//! Everything here works on a vault path so it is testable without a window.
//! Writes are atomic (temp file + rename) and guarded by a revision token so a
//! save never silently overwrites a file that changed on disk. Deleting moves a
//! board to `<vault>/.trash/boards/`, never removes it.

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const BOARDS_DIR: &str = "boards";
const TRASH_DIR: &str = ".trash/boards";
const ASSETS_DIR: &str = "assets";
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

/// Validate a board or asset base name: no separators, no dots at the start.
///
/// # Errors
/// Returns a message for empty, too long, hidden or path-like names.
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
    Ok(name.to_string())
}

fn boards_dir(vault: &Path) -> PathBuf {
    vault.join(BOARDS_DIR)
}

fn board_path(vault: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(boards_dir(vault).join(format!("{}.{EXT}", validate_name(name)?)))
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

/// Write `bytes` to `path` via a temp file in the same directory, then rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("path has no parent")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{file}.{}.tmp", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The scene a new board starts with.
#[must_use]
pub fn empty_scene() -> String {
    r##"{"type":"excalidraw","version":2,"source":"next-agent","elements":[],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##.to_string()
}

/// List boards, newest first.
///
/// # Errors
/// Returns a message on I/O failure.
pub fn list(vault: &Path) -> Result<Vec<BoardMeta>, String> {
    let dir = boards_dir(vault);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
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
    let path = board_path(vault, name)?;
    if path.exists() {
        return Err(format!("a board named \"{}\" already exists", name.trim()));
    }
    let content = scene.map_or_else(empty_scene, str::to_string);
    validate_scene(&content)?;
    atomic_write(&path, content.as_bytes())?;
    open(vault, name)
}

/// Read a board.
///
/// # Errors
/// Returns a message if the board does not exist or cannot be read.
pub fn open(vault: &Path, name: &str) -> Result<BoardDoc, String> {
    let path = board_path(vault, name)?;
    let meta = fs::metadata(&path).map_err(|_| format!("board \"{}\" not found", name.trim()))?;
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(BoardDoc {
        name: validate_name(name)?,
        content,
        revision: revision_of(&meta),
    })
}

/// Save a board atomically. When `base_revision` is given and the file on disk
/// has a different revision, the save is refused with a `conflict:` error and
/// nothing is written. Returns the new revision.
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
    let path = board_path(vault, name)?;
    if let (Some(base), Ok(meta)) = (base_revision, fs::metadata(&path)) {
        if revision_of(&meta) != base {
            return Err(format!(
                "conflict: board \"{}\" changed on disk since it was opened",
                name.trim()
            ));
        }
    }
    atomic_write(&path, content.as_bytes())?;
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    Ok(revision_of(&meta))
}

/// Rename a board. Fails if the target exists.
///
/// # Errors
/// Returns a message if the source is missing or the target exists.
pub fn rename(vault: &Path, from: &str, to: &str) -> Result<BoardMeta, String> {
    let src = board_path(vault, from)?;
    let dst = board_path(vault, to)?;
    if !src.is_file() {
        return Err(format!("board \"{}\" not found", from.trim()));
    }
    if dst.exists() {
        return Err(format!("a board named \"{}\" already exists", to.trim()));
    }
    fs::rename(&src, &dst).map_err(|e| e.to_string())?;
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
    let src = board_path(vault, name)?;
    if !src.is_file() {
        return Err(format!("board \"{}\" not found", name.trim()));
    }
    let trash = vault.join(TRASH_DIR);
    fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let base = validate_name(name)?;
    let mut target = trash.join(format!("{base}-{stamp}.{EXT}"));
    let mut n = 1;
    while target.exists() {
        target = trash.join(format!("{base}-{stamp}-{n}.{EXT}"));
        n += 1;
    }
    fs::rename(&src, &target).map_err(|e| e.to_string())?;
    let relative = target.strip_prefix(vault).unwrap_or(&target);
    Ok(relative.display().to_string())
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
    let dir = vault.join(ASSETS_DIR);
    let mut target = dir.join(format!("{base}.{ext}"));
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{base}-{n}.{ext}"));
        n += 1;
    }
    atomic_write(&target, &bytes)?;
    let relative = target.strip_prefix(vault).unwrap_or(&target);
    Ok(relative.display().to_string())
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
}
