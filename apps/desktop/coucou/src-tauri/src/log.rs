// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/log.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Next Agent: Coucou wrote its own free-text file, `next-agent.log`, beside its
// settings. It now emits `tracing` events with target `overlay`; the desktop
// app's subscriber (crates/diagnostics, ADR-0013) writes them to the one
// `logs/` folder, keeps only allowlisted fields and redacts every string.
// Only `error` and `reason` may hold free text, so these helpers put their text
// there. A message that could carry what a person works on (chat, dictation or
// notes text, window titles, URLs, file or project names) must not reach them:
// call sites log an event name plus allowlisted fields instead
// (docs/upstream/Coucou.md has the audit).

use std::fmt::Display;

use crate::settings;

/// An informational event with a short constant `message` (safe text only: it
/// goes into the redacted `reason` field). `event` is its stable snake_case name.
pub fn line(event: &'static str, message: impl Display) {
    tracing::info!(target: "overlay", event = event, reason = %message);
}

/// A failure: `event` plus the error's text in the redacted `error` field.
/// Pass a constant instead of the error when it could echo user content.
pub fn fail(event: &'static str, error: impl Display) {
    tracing::warn!(target: "overlay", event = event, error = %error);
}

/// What the island's `log_line` command sends: free text from the web page,
/// which may name a session or a file. Only a leading lowercase word survives,
/// as the `kind` of a `ui_note` event ("decide", "drag", "drop"); the rest is dropped.
pub fn ui_note(message: &str) {
    tracing::info!(target: "overlay", kind = ui_kind(message), "ui_note");
}

fn ui_kind(message: &str) -> &str {
    let word = message.split_whitespace().next().unwrap_or("").trim_end_matches(':');
    if !word.is_empty() && word.len() <= 24 && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        word
    } else {
        "other"
    }
}

/// The old `overlay/next-agent.log` is retired: delete it. Safe to call at
/// every start; it does nothing once the file is gone.
pub fn retire_old_file() {
    let _ = std::fs::remove_file(settings::local_dir().join("next-agent.log"));
}

#[cfg(test)]
mod tests {
    use super::ui_kind;

    #[test]
    fn the_island_text_is_reduced_to_its_first_word() {
        assert_eq!(ui_kind("decide allow req=abc"), "decide");
        assert_eq!(ui_kind("drop: 2 file(s) into My Secret Project"), "drop");
        assert_eq!(ui_kind("/Users/x/notes.md opened"), "other");
        assert_eq!(ui_kind(""), "other");
    }
}
