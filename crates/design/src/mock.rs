//! An offline `design` model so the whole studio works with no provider
//! configured (demos, screenshots, tests). It reads the same prompts a real
//! model gets and answers deterministically, using the project's tokens.

use crate::prompts::{DIMENSIONS, POSTURES};
use crate::studio::{DesignModel, Turn};
use std::fmt::Write as _;

#[derive(Debug, Default, Clone, Copy)]
pub struct MockDesignModel;

/// The fenced css block after `## tokens.css` in a system prompt.
fn tokens_from(system: &str) -> String {
    system
        .split("## tokens.css")
        .nth(1)
        .and_then(|rest| rest.split("```css").nth(1))
        .and_then(|rest| rest.split("```").next())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The first `:root { … }` block of the tokens.
fn root_block(tokens: &str) -> String {
    let Some(start) = tokens.find(":root") else {
        return String::new();
    };
    let mut depth = 0;
    for (i, ch) in tokens[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return tokens[start..=start + i].to_string();
                }
            }
            _ => {}
        }
    }
    tokens[start..].to_string()
}

/// `var(--name, fallback)` for the first token whose name contains a key.
fn pick(tokens: &str, keys: &[&str], fallback: &str) -> String {
    for key in keys {
        for line in tokens.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("--").and_then(|l| l.split(':').next()) {
                if name == *key || name.contains(key) {
                    return format!("var(--{name}, {fallback})");
                }
            }
        }
    }
    fallback.to_string()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Palette {
    bg: String,
    panel: String,
    text: String,
    muted: String,
    accent: String,
    border: String,
}

impl Palette {
    fn from(tokens: &str) -> Self {
        Self {
            bg: pick(tokens, &["bg", "background", "canvas"], "#fafafa"),
            panel: pick(tokens, &["panel", "surface", "card"], "#ffffff"),
            text: pick(tokens, &["text", "fg", "foreground"], "#1b1d21"),
            muted: pick(tokens, &["muted", "subtle"], "#666b75"),
            accent: pick(tokens, &["accent", "primary", "brand"], "#3b5bdb"),
            border: pick(tokens, &["border", "line"], "#dcdee3"),
        }
    }
}

fn direction_html(posture: &str, brief: &str, tokens: &str, mobile: bool) -> String {
    let p = Palette::from(tokens);
    let root = root_block(tokens);
    let title = escape(brief.lines().next().unwrap_or("Untitled").trim());
    let (radius, gap, heading, layout) = match posture {
        "tech-utility" => ("4px", "8px", "18px", "table"),
        "human-approachable" => ("16px", "20px", "28px", "cards"),
        "editorial-monocle" => ("0", "24px", "40px", "editorial"),
        "brutalist-experimental" => ("0", "12px", "56px", "bold"),
        _ => ("8px", "16px", "24px", "cards"),
    };
    let cols = if mobile {
        "1fr"
    } else {
        "repeat(3, minmax(0, 1fr))"
    };
    let body = match layout {
        "table" => format!(
            "<table><thead><tr><th>Name</th><th>Status</th><th>Updated</th></tr></thead><tbody>{}</tbody></table>",
            ["Overview", "Activity", "Settings", "Members"].iter().enumerate().fold(String::new(), |mut acc, (i, n)| { let _ = write!(acc, "<tr><td>{n}</td><td><span class=\"pill\">{}</span></td><td class=\"mono\">{}m ago</td></tr>", if i % 2 == 0 { "Active" } else { "Paused" }, (i + 1) * 7); acc })
        ),
        "editorial" => "<article><p class=\"lede\">A clear first sentence that says what this screen is for and what to do next.</p><hr><p>Supporting detail sits in a readable column with generous line height, separated by rules rather than boxes.</p></article>".to_string(),
        _ => format!(
            "<div class=\"grid\">{}</div>",
            ["Get started", "Recent work", "Shortcuts"].iter().fold(String::new(), |mut acc, n| { let _ = write!(acc, "<section class=\"card\"><h2>{n}</h2><p>Short, specific description of {}.</p><button>Open</button></section>", n.to_lowercase()); acc })
        ),
    };
    format!(
        "<!doctype html>
<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>
<style>
{root}
* {{ box-sizing: border-box; }}
body {{ margin: 0; font-family: inherit; background: {bg}; color: {text}; }}
header {{ display: flex; align-items: center; justify-content: space-between; padding: {gap}; border-bottom: 1px solid {border}; background: {panel}; }}
h1 {{ font-size: {heading}; margin: 0; letter-spacing: -0.01em; }}
main {{ padding: {gap}; display: grid; gap: {gap}; }}
.grid {{ display: grid; grid-template-columns: {cols}; gap: {gap}; }}
.card {{ background: {panel}; border: 1px solid {border}; border-radius: {radius}; padding: {gap}; display: grid; gap: 8px; }}
.card h2 {{ font-size: 15px; margin: 0; }}
p {{ color: {muted}; margin: 0; line-height: 1.5; }}
.lede {{ font-size: 20px; color: {text}; max-width: 60ch; }}
hr {{ border: 0; border-top: 1px solid {border}; }}
table {{ width: 100%; border-collapse: collapse; background: {panel}; }}
th, td {{ text-align: left; padding: 6px 8px; border-bottom: 1px solid {border}; }}
th {{ font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: {muted}; }}
.mono {{ font-family: ui-monospace, monospace; color: {muted}; }}
.pill {{ border: 1px solid {accent}; color: {accent}; border-radius: 999px; padding: 0 8px; font-size: 12px; }}
button {{ font: inherit; border-radius: {radius}; border: 1px solid {border}; background: {panel}; color: {text}; padding: 6px 12px; justify-self: start; }}
button.primary {{ background: {accent}; border-color: {accent}; color: #fff; }}
button:focus-visible {{ outline: 2px solid {accent}; outline-offset: 2px; }}
</style></head>
<body><header><h1>{title}</h1><button class=\"primary\">New</button></header>
<main>{body}</main></body></html>",
        bg = p.bg, panel = p.panel, text = p.text, muted = p.muted, accent = p.accent, border = p.border,
    )
}

fn brief_of(turns: &[Turn]) -> String {
    turns
        .last()
        .map_or_default(|t| t.text.split("\n\n").next().unwrap_or("").to_string())
}

impl DesignModel for MockDesignModel {
    #[allow(clippy::too_many_lines)] // one branch per prompt kind
    fn complete(&self, system: &str, turns: &[Turn]) -> Result<String, String> {
        let tokens = tokens_from(system);
        let last = turns.last().map_or_default(|t| t.text.as_str());
        if system.contains("clearly different directions") {
            let mobile = system.contains("390×844");
            let mut out = String::new();
            for posture in POSTURES
                .iter()
                .filter(|p| system.contains(&format!("`{}`", p.id)))
            {
                let brief = brief_of(turns);
                let _ = write!(out,
                    "Direction: {} — {} (offline mock design model).\n<artifact identifier=\"{}\" type=\"text/html\" title=\"{}\">\n{}\n</artifact>\n",
                    posture.label, posture.brief, posture.id, posture.label, direction_html(posture.id, &brief, &tokens, mobile)
                );
            }
            return Ok(out);
        }
        if system.contains("You are the design reviewer") {
            let weak = last.to_ascii_lowercase().contains("lorem ipsum");
            let scores = DIMENSIONS
                .iter()
                .enumerate()
                .map(|(i, (name, _))| {
                    format!(
                        "\"{name}\":{}",
                        if weak && i == 3 {
                            2
                        } else {
                            4 - u8::from(i == 4)
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            return Ok(format!("{{{scores},\"notes\":\"Mock review: restraint could go further; one accent per screen.\"}}"));
        }
        if system.contains("refining one chosen design") {
            let html = last
                .split("type=\"text/html\"")
                .nth(1)
                .and_then(|r| r.split_once('>'))
                .map(|(_, r)| r)
                .and_then(|r| r.split("</artifact>").next())
                .unwrap_or_default()
                .trim();
            let change = last.rsplit("Change: ").next().unwrap_or_default().trim();
            let note = format!("<p style=\"margin:0;padding:8px 16px;border-top:1px solid currentColor;opacity:.7\">Revised: {}</p>", escape(change));
            let updated = html.rfind("</body>").map_or_else(
                || format!("{html}{note}"),
                |i| format!("{}{note}{}", &html[..i], &html[i..]),
            );
            let id = last
                .split("identifier=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or("direction");
            return Ok(format!("Applied “{}” (offline mock).\n<artifact identifier=\"{id}\" type=\"text/html\" title=\"Revised\">\n{updated}\n</artifact>", escape(change)));
        }
        if system.contains("specializes in") {
            let html = last
                .split("Given the following HTML:")
                .nth(1)
                .and_then(|r| r.split("Please turn this").next())
                .unwrap_or_default();
            let body = html
                .split("<body>")
                .nth(1)
                .and_then(|r| r.split("</body>").next())
                .unwrap_or(html)
                .trim();
            let style = html
                .split("<style>")
                .nth(1)
                .and_then(|r| r.split("</style>").next())
                .unwrap_or_default();
            let style = match (style.find(":root"), style.find("}\n")) {
                (Some(s), Some(e)) if e > s => format!("{}{}", &style[..s], &style[e + 2..]),
                _ => style.to_string(),
            };
            let file = system
                .split("identifier=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or("Design.svelte");
            let ext = std::path::Path::new(file)
                .extension()
                .map_or_else(String::new, |e| e.to_string_lossy().to_ascii_lowercase());
            let code = if ext == "svelte" {
                format!("<!-- Converted by the offline mock design model. -->\n<script lang=\"ts\">\n  let {{ title = \"\" }}: {{ title?: string }} = $props();\n</script>\n\n<div class=\"design-root\">\n{body}\n</div>\n\n<style>\n{}\n</style>\n", style.replace("body {", ".design-root {").trim())
            } else if ext == "tsx" {
                let jsx = body
                    .replace("class=", "className=")
                    .replace("for=", "htmlFor=");
                let name = file.trim_end_matches(".tsx");
                format!("// Converted by the offline mock design model.\nconst css = `{}`;\n\nexport default function {name}() {{\n  return (\n    <div className=\"design-root\">\n      <style>{{css}}</style>\n      {jsx}\n    </div>\n  );\n}}\n", style.replace('`', "\\`").trim())
            } else if ext == "vue" {
                format!("<!-- Converted by the offline mock design model. -->\n<template>\n  <div class=\"design-root\">\n{body}\n  </div>\n</template>\n\n<style scoped>\n{}\n</style>\n", style.trim())
            } else {
                let tag = file.trim_end_matches(".js");
                format!("// Converted by the offline mock design model.\nclass DesignView extends HTMLElement {{\n  connectedCallback() {{\n    const root = this.attachShadow({{ mode: \"open\" }});\n    root.innerHTML = `<style>{}</style>{}`;\n  }}\n}}\ncustomElements.define(\"{tag}\", DesignView);\n", style.replace('`', "\\`").trim(), body.replace('`', "\\`"))
            };
            let mime = system
                .split("type=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or("text/plain");
            return Ok(format!("<artifact identifier=\"{file}\" type=\"{mime}\" title=\"{file}\">\n{code}</artifact>"));
        }
        Err("the offline mock design model does not understand this request".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::Framework;
    use crate::studio::{self, Frame, GenerateRequest, IterateRequest};

    const TOKENS: &str =
        ":root {\n  --bg: #fafafa;\n  --panel: #fff;\n  --text: #111;\n  --accent: #3b5bdb;\n}\n";

    #[test]
    fn mock_runs_the_whole_flow() {
        let m = MockDesignModel;
        let req = GenerateRequest {
            prompt: "Team settings",
            frame: Frame::Mobile,
            count: 3,
            design_md: "# DS",
            tokens_css: TOKENS,
            sketch: None,
            critique: true,
        };
        let dirs = studio::generate(&m, &req, &mut |_| {}).expect("generate");
        assert_eq!(dirs.len(), 3);
        assert_eq!(dirs[0].identifier, "modern-minimal");
        assert!(dirs[0].html.contains("var(--accent, #3b5bdb)"));
        assert!(dirs[0].html.contains("<title>Team settings</title>"));
        assert!(dirs.iter().all(|d| d
            .critique
            .as_ref()
            .is_some_and(crate::critique::Critique::passes)));
        assert!(
            dirs.iter().all(|d| !d
                .lint
                .iter()
                .any(|f| f.severity == crate::lint::Severity::P0)),
            "{:?}",
            dirs[0].lint
        );

        let (v2, summary) = studio::iterate(
            &m,
            &IterateRequest {
                design_md: "",
                tokens_css: TOKENS,
                frame: Frame::Mobile,
                current: &dirs[1],
                history: &[],
                message: "make <it> calmer",
            },
        )
        .expect("iterate");
        assert!(v2.html.contains("Revised: make &lt;it&gt; calmer"));
        assert!(summary.contains("offline mock"));

        let dir = tempfile::tempdir().expect("tmp");
        for (fw, ext) in [
            (Framework::Svelte, ".svelte"),
            (Framework::React, ".tsx"),
            (Framework::Vue, ".vue"),
            (Framework::WebComponent, ".js"),
        ] {
            let c = studio::convert(&m, dir.path(), &dirs[0], fw, "").expect("convert");
            assert!(c.path.ends_with(ext), "{}", c.path);
            assert!(c.contents.contains("Converted by the offline mock"));
        }
    }

    #[test]
    fn unknown_requests_fail() {
        assert!(MockDesignModel.complete("hello", &[]).is_err());
    }
}
