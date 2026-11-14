// Ported from OpenClaw (MIT, © 2026 OpenClaw Foundation): src/auto-reply/chunk.ts (chunkText, chunkMarkdownText) @ 1ac07b333581ff06e7d323015dc5912247415d4b
//! Split long replies to fit a channel's message limit.
//!
//! Lengths are UTF-16 code units (Telegram counts that way), and a cut never
//! splits a character. Inside a window the cut prefers the last newline,
//! then the last whitespace, then a hard cut. A cut inside a fenced code
//! block closes the fence at the end of the chunk and reopens it at the
//! start of the next, so each chunk renders on its own.

/// Telegram's `sendMessage` text limit.
pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Byte index of the longest prefix of `text` within `limit` UTF-16 units.
fn prefix_end(text: &str, limit: usize) -> usize {
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        used += ch.len_utf16();
        if used > limit {
            return index;
        }
    }
    text.len()
}

/// Where to cut `window` (a byte index > 0), preferring newline, then
/// whitespace. `None` = no soft break.
fn soft_break(window: &str) -> Option<usize> {
    if let Some(i) = window.rfind('\n').filter(|i| *i > 0) {
        return Some(i);
    }
    window
        .char_indices()
        .rev()
        .find(|(i, c)| *i > 0 && c.is_whitespace())
        .map(|(i, _)| i)
}

/// The fence line (`` ``` `` or `~~~` plus info) still open at the end of
/// `text`, if any.
fn open_fence(text: &str) -> Option<String> {
    let mut open: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            "```"
        } else if trimmed.starts_with("~~~") {
            "~~~"
        } else {
            continue;
        };
        match &open {
            Some(current) if current.trim_start().starts_with(marker) => open = None,
            Some(_) => {}
            None => open = Some(trimmed.to_string()),
        }
    }
    open
}

/// Split `text` into chunks of at most `limit` UTF-16 units each.
#[must_use]
pub fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    // Room for a closing fence (`\n` + marker) and a reopen line.
    let limit = limit.max(16);
    let mut chunks = Vec::new();
    let mut reopen = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let prefix = if reopen.is_empty() {
            String::new()
        } else {
            format!("{reopen}\n")
        };
        let budget = limit.saturating_sub(units(&prefix));
        if units(rest) <= budget {
            chunks.push(format!("{prefix}{rest}"));
            break;
        }
        // Reserve room to close a fence that might be open at the cut.
        let close_room = 4;
        let hard = prefix_end(rest, budget.saturating_sub(close_room).max(1));
        let hard = if hard == 0 {
            rest.chars().next().map_or(rest.len(), char::len_utf8)
        } else {
            hard
        };
        let cut = soft_break(&rest[..hard]).unwrap_or(hard);
        let (head, tail) = rest.split_at(cut);
        let mut chunk = format!("{prefix}{}", head.trim_end_matches(['\n', ' ']));
        reopen = match open_fence(&chunk) {
            Some(fence) => {
                let marker = if fence.starts_with('~') { "~~~" } else { "```" };
                chunk.push('\n');
                chunk.push_str(marker);
                fence
            }
            None => String::new(),
        };
        chunks.push(chunk);
        rest = tail.trim_start_matches(['\n', ' ']);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunk_text("hello", 4096), ["hello"]);
        assert!(chunk_text("", 4096).is_empty());
    }

    #[test]
    fn chunks_respect_the_telegram_limit() {
        let text = "word ".repeat(3_000);
        let chunks = chunk_text(&text, TELEGRAM_TEXT_LIMIT);
        assert!(chunks.len() >= 4);
        for chunk in &chunks {
            assert!(units(chunk) <= TELEGRAM_TEXT_LIMIT, "{}", units(chunk));
            assert!(!chunk.starts_with(' '));
        }
        assert_eq!(chunks.join(" ").split_whitespace().count(), 3_000);
    }

    #[test]
    fn exactly_at_the_limit_is_not_split() {
        let text = "a".repeat(TELEGRAM_TEXT_LIMIT);
        assert_eq!(chunk_text(&text, TELEGRAM_TEXT_LIMIT).len(), 1);
        let text = "a".repeat(TELEGRAM_TEXT_LIMIT + 1);
        assert_eq!(chunk_text(&text, TELEGRAM_TEXT_LIMIT).len(), 2);
    }

    #[test]
    fn prefers_newlines_over_spaces() {
        let text = format!("{}\n{}", "a ".repeat(20), "b ".repeat(20));
        let chunks = chunk_text(&text, 50);
        assert!(chunks[0].ends_with('a'));
        assert!(chunks[1].starts_with('b'));
    }

    #[test]
    fn never_splits_surrogate_pairs() {
        let text = "😀".repeat(3_000); // 2 UTF-16 units each
        let chunks = chunk_text(&text, TELEGRAM_TEXT_LIMIT);
        for chunk in &chunks {
            assert!(units(chunk) <= TELEGRAM_TEXT_LIMIT);
            assert!(chunk.chars().all(|c| c == '😀'));
        }
        assert_eq!(chunks.concat().chars().count(), 3_000);
    }

    #[test]
    fn hard_cuts_a_long_word() {
        let text = "x".repeat(130);
        let chunks = chunk_text(&text, 50);
        assert!(chunks.iter().all(|c| units(c) <= 50));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn code_fences_are_closed_and_reopened() {
        let code = (0..60).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let text = format!("Here:\n```rust\n{code}\n```\nDone.");
        let chunks = chunk_text(&text, 120);
        assert!(chunks.len() > 2);
        for chunk in &chunks {
            assert!(units(chunk) <= 120, "{chunk:?}");
            assert_eq!(chunk.matches("```").count() % 2, 0, "unbalanced: {chunk:?}");
        }
        assert!(chunks[1].starts_with("```rust\n"));
        assert!(chunks.last().is_some_and(|c| c.ends_with("Done.")));
    }
}
