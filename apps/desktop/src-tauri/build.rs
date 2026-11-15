//! Declares every command in Tauri's app manifest, so each window may call
//! only what its capability grants (`capabilities/*.json`, written by
//! `scripts/gen-capabilities.mjs`). Without a manifest Tauri lets every window
//! call every app command: the overlay's windows could call Next Agent's
//! `rule_delete` or the gateway's secrets, and the main window the overlay's.

use std::path::Path;

/// Command names inside `macro![ … ]`: the last path segment of each entry.
fn command_list(source: &str, macro_name: &str) -> Vec<String> {
    let open = format!("{macro_name}[");
    let start = source
        .find(&open)
        .unwrap_or_else(|| panic!("no {open} … ] found"))
        + open.len();
    let end = start + source[start..].find(']').expect("unclosed command list");
    source[start..end]
        .lines()
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
