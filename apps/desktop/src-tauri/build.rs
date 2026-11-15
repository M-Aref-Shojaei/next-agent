//! Declares every command in Tauri's app manifest, so each window may call
//! only what its capability grants (`capabilities/*.json`, written by
//! `scripts/gen-capabilities.mjs`). Without a manifest Tauri lets every window
//! call every app command: the overlay's windows could call Next Agent's
//! `rule_delete` or the gateway's secrets, and the main window the overlay's.

use std::path::Path;

/// Command names inside `macro![ … ]`: the last path segment of each entry.
/// Brackets are matched, so an attribute such as `#[cfg(…)]` on an entry
/// neither ends the list nor hides the entries after it (the name is kept:
/// a permission for a command a platform leaves out is harmless).
/// `scripts/gen-capabilities.mjs` reads the lists the same way.
fn command_list(source: &str, macro_name: &str) -> Vec<String> {
    let open = format!("{macro_name}[");
    let start = source
        .find(&open)
        .unwrap_or_else(|| panic!("no {open} … ] found"))
        + open.len();
    let mut depth = 0usize;
    let mut body = String::new();
    let mut attribute = 0usize;
    let mut chars = source[start..].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '#' if chars.peek() == Some(&'[') => {
                chars.next();
                attribute = 1;
                continue;
            }
            '[' if attribute > 0 => attribute += 1,
            ']' if attribute > 0 => {
                attribute -= 1;
                continue;
            }
            '[' => depth += 1,
            ']' if depth == 0 => break,
            ']' => depth -= 1,
            _ => {}
        }
        if attribute == 0 {
            body.push(c);
        }
    }
    body.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.rsplit("::").next().unwrap_or(entry).to_string())
        .collect()
}

fn read(path: &str) -> String {
    println!("cargo:rerun-if-changed={path}");
    std::fs::read_to_string(Path::new(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn main() {
    let mut commands = command_list(&read("src/lib.rs"), "generate_handler!");
    commands.extend(command_list(
        &read("../coucou/src-tauri/src/lib.rs"),
        "coucou_commands!",
    ));
    let commands: &'static [&'static str] = Vec::leak(
        commands
            .into_iter()
            .map(|c| &*String::leak(c))
            .collect::<Vec<_>>(),
    );
    println!("cargo:rerun-if-changed=capabilities");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}
