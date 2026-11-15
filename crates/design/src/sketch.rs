//! Turn a Design board sketch (`board_sketch`: live Excalidraw elements)
//! into a short text wireframe the `design` model can follow.

use serde_json::Value;

const MAX_ITEMS: usize = 80;

/// One line per element, top-to-bottom then left-to-right, relative to the
/// sketch's top-left corner. Text and labels are kept; free-hand strokes and
/// images are summarised.
#[must_use]
pub fn describe(elements: &[Value]) -> String {
    let num = |e: &Value, k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let items: Vec<&Value> = elements
        .iter()
        .filter(|e| e.get("isDeleted").and_then(Value::as_bool) != Some(true))
        .collect();
    if items.is_empty() {
        return String::new();
    }
    let min_x = items
        .iter()
        .map(|e| num(e, "x"))
        .fold(f64::INFINITY, f64::min);
    let min_y = items
        .iter()
        .map(|e| num(e, "y"))
        .fold(f64::INFINITY, f64::min);
    let mut sorted = items.clone();
    sorted.sort_by(|a, b| {
        let row = |e: &Value| ((num(e, "y") - min_y) / 24.0).floor();
        row(a)
            .total_cmp(&row(b))
            .then(num(a, "x").total_cmp(&num(b, "x")))
    });
    let mut lines = Vec::new();
    let mut strokes = 0;
    for e in sorted.iter().take(MAX_ITEMS) {
        let kind = e.get("type").and_then(Value::as_str).unwrap_or("shape");
        #[allow(clippy::cast_possible_truncation)]
        let (x, y, w, h) = (
            (num(e, "x") - min_x).round() as i64,
            (num(e, "y") - min_y).round() as i64,
            num(e, "width").round() as i64,
            num(e, "height").round() as i64,
        );
        let text = e
            .get("text")
            .or_else(|| e.get("originalText"))
            .and_then(Value::as_str)
            .map(|t| t.replace('\n', " ").chars().take(120).collect::<String>());
        match kind {
            "freedraw" => strokes += 1,
            "text" => lines.push(format!(
                "- text \"{}\" at ({x}, {y})",
                text.unwrap_or_default()
            )),
            "frame" | "magicframe" => {
                let name = e.get("name").and_then(Value::as_str).unwrap_or("frame");
                lines.push(format!("- frame \"{name}\" at ({x}, {y}) size {w}×{h}"));
            }
            "arrow" | "line" => lines.push(format!("- {kind} from ({x}, {y}) spanning {w}×{h}")),
            other => {
                let label = text.map_or_default(|t| format!(" labelled \"{t}\""));
                lines.push(format!("- {other} at ({x}, {y}) size {w}×{h}{label}"));
            }
        }
    }
    if strokes > 0 {
        lines.push(format!("- plus {strokes} free-hand stroke(s)"));
    }
    if items.len() > MAX_ITEMS {
        lines.push(format!("- … and {} more elements", items.len() - MAX_ITEMS));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn describes_in_reading_order() {
        let els = vec![
            json!({"type":"rectangle","x":110,"y":200,"width":300,"height":40,"text":"Email"}),
            json!({"type":"text","x":100,"y":100,"text":"Sign in\nnow"}),
            json!({"type":"freedraw","x":100,"y":500}),
            json!({"type":"ellipse","x":100,"y":300,"width":10,"height":10,"isDeleted":true}),
            json!({"type":"frame","x":100,"y":70,"width":400,"height":500,"name":"Login"}),
        ];
        let d = describe(&els);
        let lines: Vec<&str> = d.lines().collect();
        assert_eq!(lines[0], "- frame \"Login\" at (0, 0) size 400×500");
        assert_eq!(lines[1], "- text \"Sign in now\" at (0, 30)");
        assert!(lines[2].starts_with("- rectangle at (10, 130) size 300×40 labelled \"Email\""));
        assert_eq!(lines.last().copied(), Some("- plus 1 free-hand stroke(s)"));
        assert!(!d.contains("ellipse"));
        assert_eq!(describe(&[]), "");
    }
}
