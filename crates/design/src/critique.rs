// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// packages/contracts/src/prompts/discovery.ts (Step 8, five-dimensional
// critique: any dimension under 3/5 is a regression) @ 59e4f2a — modified:
// the scores are returned as JSON by a separate review pass and kept on the
// direction so the user sees them; a revision replaces the direction.
//! Parse the self-critique pass.

use crate::artifact::{self, Artifact};
use crate::prompts::DIMENSIONS;
use serde::{Deserialize, Serialize};

/// Scores (1–5) per dimension, in [`DIMENSIONS`] order, plus notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Critique {
    pub scores: Vec<(String, u8)>,
    pub notes: String,
    /// True when the reviewer replaced the direction with a fixed version.
    #[serde(default)]
    pub revised: bool,
}

impl Critique {
    /// Lowest score (0 when none parsed).
    #[must_use]
    pub fn weakest(&self) -> u8 {
        self.scores.iter().map(|(_, s)| *s).min().unwrap_or(0)
    }

    /// Every dimension at 3 or above.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.scores.len() == DIMENSIONS.len() && self.weakest() >= 3
    }
}

/// The first balanced `{ … }` in `text` (skipping strings), if any.
fn first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in text[start..].char_indices() {
        if in_str {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read the reviewer's reply: the JSON scores and an optional revision.
/// Missing or unparseable scores give `None` (the direction is kept as is).
#[must_use]
pub fn parse(text: &str) -> Option<(Critique, Option<Artifact>)> {
    let json: serde_json::Value = serde_json::from_str(first_json_object(text)?).ok()?;
    let mut scores = Vec::new();
    for (name, _) in DIMENSIONS {
        let score = json.get(*name).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        scores.push(((*name).to_string(), score.round().clamp(1.0, 5.0) as u8));
    }
    let notes = json
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .chars()
        .take(400)
        .collect();
    let revision = artifact::parse(text)
        .into_iter()
        .find(|a| !a.truncated && !a.content.is_empty());
    let critique = Critique {
        scores,
        notes,
        revised: false,
    };
    // A revision only counts when it was asked for (some score < 3).
    let revision = if critique.passes() { None } else { revision };
    Some((critique, revision))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scores_and_revision() {
        let text = "{\"philosophy\":4,\"hierarchy\":2,\"execution\":\"3\",\"specificity\":4.4,\"restraint\":5,\"notes\":\"CTA competes with {nav}\"}\n<artifact identifier=\"a\" title=\"A\"><html>fixed</html></artifact>";
        let (c, rev) = parse(text).expect("parsed");
        assert_eq!(c.scores[1], ("hierarchy".into(), 2));
        assert_eq!(c.scores[3].1, 4);
        assert_eq!(c.weakest(), 2);
        assert!(!c.passes());
        assert_eq!(c.notes, "CTA competes with {nav}");
        assert_eq!(rev.expect("revision").content, "<html>fixed</html>");
    }

    #[test]
    fn passing_scores_ignore_stray_artifacts() {
        let text = "```json\n{\"philosophy\":3,\"hierarchy\":4,\"execution\":4,\"specificity\":3,\"restraint\":9}\n```<artifact>x</artifact>";
        let (c, rev) = parse(text).expect("parsed");
        assert!(c.passes());
        assert_eq!(c.scores[4].1, 5, "clamped");
        assert!(rev.is_none());
    }

    #[test]
    fn missing_dimensions_or_json_give_none() {
        assert!(parse("looks fine").is_none());
        assert!(parse("{\"philosophy\":4}").is_none());
        assert!(parse("{not json}").is_none());
    }
}
