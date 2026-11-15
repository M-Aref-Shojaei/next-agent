// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// design-systems/README.md (DESIGN.md + tokens.css package layout, the
// nine-section DESIGN.md headings) @ 59e4f2a — modified: the sections are
// filled from a project's own CSS by our extractor; no design system taken.
//! Design systems: read a project's CSS into tokens, then write `DESIGN.md`
//! (the brand contract the `design` model designs from) and `tokens.css`.
//!
//! The `DESIGN.md` idea and its section layout come from Open Design
//! (Apache-2.0); see [`crate::system::render_design_md`]. The extraction
//! itself is our own.

use crate::css::{self, Declaration};
use crate::DesignError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What a token is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Color,
    Font,
    Size,
    Spacing,
    Radius,
    Shadow,
    Other,
}

impl TokenKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "Colour",
            Self::Font => "Typeface",
            Self::Size => "Type scale",
            Self::Spacing => "Spacing",
            Self::Radius => "Radius",
            Self::Shadow => "Elevation",
            Self::Other => "Other",
        }
    }
}

/// One design token: a CSS custom property with its light and dark values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// Property name including the leading `--`.
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
    pub kind: TokenKind,
    /// True when synthesised from literal values (the project had no token).
    #[serde(default)]
    pub derived: bool,
}

/// Everything extracted from a project's styles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignSystem {
    pub name: String,
    pub tokens: Vec<Token>,
    /// Colour literals by how often they appear (most used first).
    pub palette: Vec<(String, usize)>,
    pub fonts: Vec<String>,
    pub font_sizes: Vec<String>,
    pub spacing: Vec<String>,
    pub radii: Vec<String>,
    pub shadows: Vec<String>,
    /// Workspace-relative files the styles were read from.
    pub sources: Vec<String>,
}

/// Folders never scanned for styles.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    ".git",
    ".svelte-kit",
    ".next",
    ".nuxt",
    ".output",
    "coverage",
    "vendor",
    ".next-agent",
];
const STYLE_EXTS: &[&str] = &["css", "scss", "sass", "less", "pcss", "postcss"];
const MARKUP_EXTS: &[&str] = &["svelte", "vue", "html", "astro"];
const MAX_FILES: usize = 400;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_DEPTH: usize = 8;

/// Collect `(relative path, css text)` for every stylesheet and `<style>`
/// block under `root`, skipping build output, dependencies and symlinks.
///
/// # Errors
/// Returns [`DesignError::Io`] when `root` cannot be read.
pub fn collect_styles(root: &Path) -> Result<Vec<(String, String)>, DesignError> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    std::fs::read_dir(root).map_err(|e| DesignError::Io(format!("{}: {e}", root.display())))?;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            if out.len() >= MAX_FILES {
                return Ok(out);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue; // never follow links out of the workspace
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if kind.is_dir() {
                if depth < MAX_DEPTH
                    && !SKIP_DIRS.contains(&name.as_str())
                    && !name.starts_with('.')
                {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path
                .extension()
                .map_or_default(|e| e.to_string_lossy().to_ascii_lowercase());
            let is_style = STYLE_EXTS.contains(&ext.as_str());
            if !is_style && !MARKUP_EXTS.contains(&ext.as_str()) {
                continue;
            }
            if entry.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let css = if is_style {
                text
            } else {
                css::style_blocks(&text).join("\n")
            };
            if css.trim().is_empty() {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, css));
        }
    }
    Ok(out)
}

/// Read `root`'s styles into a design system.
///
/// # Errors
/// Returns [`DesignError::Io`] when `root` cannot be read.
pub fn extract(root: &Path, name: &str) -> Result<DesignSystem, DesignError> {
    Ok(extract_from_sources(name, &collect_styles(root)?))
}

/// Pure core of [`extract`]: `(relative path, css)` pairs in, tokens out.
/// Global stylesheets are read before component styles so app-wide tokens win.
#[must_use]
pub fn extract_from_sources(name: &str, sources: &[(String, String)]) -> DesignSystem {
    let mut ordered: Vec<&(String, String)> = sources.iter().collect();
    ordered.sort_by_key(|(path, _)| {
        let ext_rank = u8::from(!Path::new(path).extension().is_some_and(|e| {
            STYLE_EXTS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
        }));
        (ext_rank, path.matches('/').count(), path.clone())
    });

    let mut tokens: Vec<Token> = Vec::new();
    let mut index: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    let mut tally = Tally::default();
    for (_, text) in &ordered {
        for decl in css::declarations(text) {
            if decl.property.starts_with("--") {
                add_token(&mut tokens, &mut index, &decl);
            } else {
                tally.add(&decl);
            }
            for color in css::colors_in(&decl.value) {
                *tally.colors.entry(color).or_default() += 1;
            }
        }
    }
    // Resolve kinds through var() chains once everything is known.
    let kinds: BTreeMap<String, TokenKind> =
        tokens.iter().map(|t| (t.name.clone(), t.kind)).collect();
    for token in &mut tokens {
        if token.kind == TokenKind::Other {
            if let Some(target) = var_target(&token.value) {
                if let Some(kind) = kinds.get(target) {
                    token.kind = *kind;
                }
            }
        }
    }

    let mut system = DesignSystem {
        name: name.to_string(),
        palette: tally.ranked_colors(),
        fonts: with_first(ranked(&tally.fonts), tally.root_font.clone()),
        font_sizes: ranked(&tally.sizes),
        spacing: ranked(&tally.spacing),
        radii: ranked(&tally.radii),
        shadows: ranked(&tally.shadows),
        sources: ordered.iter().map(|(p, _)| p.clone()).collect(),
        tokens,
    };
    derive_missing(&mut system);
    system
}

fn add_token(
    tokens: &mut Vec<Token>,
    index: &mut BTreeMap<String, (usize, bool)>,
    decl: &Declaration,
) {
    let dark = decl.is_dark();
    if let Some(&(i, light_seen)) = index.get(&decl.property) {
        if dark && tokens[i].dark.is_none() {
            tokens[i].dark = Some(decl.value.clone());
        } else if !dark && !light_seen && decl.is_root_level() {
            // The dark override came first; this is the light value.
            tokens[i].value.clone_from(&decl.value);
            index.insert(decl.property.clone(), (i, true));
        }
        return;
    }
    // Custom properties scoped to a component selector are local, not tokens.
    if !decl.is_root_level() {
        return;
    }
    index.insert(decl.property.clone(), (tokens.len(), !dark));
    tokens.push(Token {
        name: decl.property.clone(),
        value: decl.value.clone(),
        dark: dark.then(|| decl.value.clone()),
        kind: classify(&decl.property, &decl.value),
        derived: false,
    });
}

fn var_target(value: &str) -> Option<&str> {
    let inner = value.trim().strip_prefix("var(")?.strip_suffix(')')?;
    Some(inner.split(',').next()?.trim())
}

/// Guess a token's kind from its value first, then its name.
#[must_use]
pub fn classify(name: &str, value: &str) -> TokenKind {
    let n = name.to_ascii_lowercase();
    if css::is_color_value(value) || n.contains("color") || n.contains("colour") {
        return TokenKind::Color;
    }
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["shadow", "elevation"]) {
        TokenKind::Shadow
    } else if has(&["radius", "rounded", "corner"]) {
        TokenKind::Radius
    } else if has(&["font-family", "family", "typeface"])
        || (n.contains("font") && value.contains(','))
    {
        TokenKind::Font
    } else if has(&["font-size", "text-", "fs-", "type-", "size"]) {
        TokenKind::Size
    } else if has(&[
        "space", "spacing", "gap", "pad", "margin", "gutter", "inset",
    ]) {
        TokenKind::Spacing
    } else {
        TokenKind::Other
    }
}

#[derive(Default)]
struct Tally {
    colors: BTreeMap<String, usize>,
    fonts: BTreeMap<String, usize>,
    sizes: BTreeMap<String, usize>,
    spacing: BTreeMap<String, usize>,
    radii: BTreeMap<String, usize>,
    shadows: BTreeMap<String, usize>,
    /// The document's own font (first `font-family` on `:root`/`html`/`body`).
    root_font: Option<String>,
}

impl Tally {
    fn add(&mut self, decl: &Declaration) {
        let bump = |map: &mut BTreeMap<String, usize>, v: &str| {
            *map.entry(v.to_string()).or_default() += 1;
        };
        let p = decl.property.as_str();
        let v = decl.value.as_str();
        if v.contains("var(") && !p.starts_with("font-family") {
            return; // already a token reference
        }
        match p {
            "font-family" if matches!(v, "inherit" | "initial" | "unset" | "revert") => {}
            "font-family" => {
                if self.root_font.is_none() && decl.is_root_level() && decl.context.len() == 1 {
                    self.root_font = Some(v.to_string());
                }
                bump(&mut self.fonts, v);
            }
            "font-size" => {
                for len in css::lengths_in(v) {
                    bump(&mut self.sizes, &len);
                }
            }
            "border-radius"
            | "border-top-left-radius"
            | "border-top-right-radius"
            | "border-bottom-left-radius"
            | "border-bottom-right-radius" => {
                for len in css::lengths_in(v) {
                    bump(&mut self.radii, &len);
                }
            }
            "box-shadow" if v != "none" => bump(&mut self.shadows, v),
            _ if is_spacing_property(p) => {
                for len in css::lengths_in(v) {
                    bump(&mut self.spacing, &len);
                }
            }
            _ => {}
        }
    }

    fn ranked_colors(&self) -> Vec<(String, usize)> {
        let mut all: Vec<(String, usize)> =
            self.colors.iter().map(|(k, v)| (k.clone(), *v)).collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        all.truncate(24);
        all
    }
}

fn is_spacing_property(p: &str) -> bool {
    p == "gap"
        || p == "row-gap"
        || p == "column-gap"
        || p.starts_with("padding")
        || p.starts_with("margin")
}

/// `first` (when known) leads the list, the rest keep their order.
fn with_first(mut list: Vec<String>, first: Option<String>) -> Vec<String> {
    if let Some(first) = first {
        list.retain(|f| *f != first);
        list.insert(0, first);
    }
    list
}

fn ranked(map: &BTreeMap<String, usize>) -> Vec<String> {
    let mut all: Vec<(&String, &usize)> = map.iter().collect();
    all.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    all.into_iter().take(12).map(|(k, _)| k.clone()).collect()
}

/// Fill categories the project has no tokens for from its literal values,
/// so `tokens.css` always gives the model a complete vocabulary.
fn derive_missing(system: &mut DesignSystem) {
    let has = |kind: TokenKind, s: &DesignSystem| s.tokens.iter().any(|t| t.kind == kind);
    let mut extra = Vec::new();
    if !has(TokenKind::Color, system) {
        for (i, (color, _)) in system.palette.iter().take(8).enumerate() {
            extra.push(derived(
                &format!("--color-{}", i + 1),
                color,
                TokenKind::Color,
            ));
        }
    }
    if !has(TokenKind::Font, system) {
        if let Some(font) = system.fonts.first() {
            extra.push(derived("--font-sans", font, TokenKind::Font));
        }
    }
    let mut numeric = |kind: TokenKind, values: &[String], prefix: &str| {
        if has(kind, system) {
            return;
        }
        let mut sorted: Vec<&String> = values.iter().take(6).collect();
        sorted.sort_by(|a, b| px(a).total_cmp(&px(b)));
        for (i, v) in sorted.into_iter().enumerate() {
            extra.push(derived(&format!("--{prefix}-{}", i + 1), v, kind));
        }
    };
    numeric(TokenKind::Size, &system.font_sizes, "text");
    numeric(TokenKind::Spacing, &system.spacing, "space");
    numeric(TokenKind::Radius, &system.radii, "radius");
    if !has(TokenKind::Shadow, system) {
        if let Some(shadow) = system.shadows.first() {
            extra.push(derived("--shadow-1", shadow, TokenKind::Shadow));
        }
    }
    system.tokens.extend(extra);
}

fn derived(name: &str, value: &str, kind: TokenKind) -> Token {
    Token {
        name: name.into(),
        value: value.into(),
        dark: None,
        kind,
        derived: true,
    }
}

/// Approximate pixels for sorting (`1rem` = 16px).
fn px(len: &str) -> f64 {
    let (num, scale) = if let Some(n) = len.strip_suffix("rem") {
        (n, 16.0)
    } else if let Some(n) = len.strip_suffix("em") {
        (n, 16.0)
    } else if let Some(n) = len.strip_suffix("px") {
        (n, 1.0)
    } else {
        (len.trim_end_matches(char::is_alphabetic), 1.0)
    };
    num.parse::<f64>().unwrap_or(0.0) * scale
}

/// The nine sections of a `DESIGN.md`, in order. Layout from Open Design's
/// `design-systems/README.md` (Apache-2.0).
pub const DESIGN_MD_SECTIONS: &[&str] = &[
    "Visual Theme & Atmosphere",
    "Color Palette & Roles",
    "Typography Rules",
    "Component Stylings",
    "Layout Principles",
    "Depth & Elevation",
    "Do's and Don'ts",
    "Responsive Behavior",
    "Agent Prompt Guide",
];

/// `tokens.css`: every token as a custom property, dark values in a
/// `prefers-color-scheme` block.
#[must_use]
pub fn render_tokens_css(system: &DesignSystem) -> String {
    let mut out = format!(
        "/* {} design tokens. Generated by the Next Agent Design Studio from the\n   project's CSS; edit freely. Directions and conversions use these names. */\n:root {{\n",
        system.name
    );
    let mut kinds: Vec<TokenKind> = system.tokens.iter().map(|t| t.kind).collect();
    kinds.sort();
    kinds.dedup();
    for kind in &kinds {
        let _ = writeln!(out, "  /* {} */", kind.label());
        for t in system.tokens.iter().filter(|t| t.kind == *kind) {
            let _ = writeln!(out, "  {}: {};", t.name, t.value);
        }
    }
    out.push_str("}\n");
    let dark: Vec<&Token> = system.tokens.iter().filter(|t| t.dark.is_some()).collect();
    if !dark.is_empty() {
        out.push_str("@media (prefers-color-scheme: dark) {\n  :root {\n");
        for t in dark {
            let _ = writeln!(
                out,
                "    {}: {};",
                t.name,
                t.dark.as_deref().unwrap_or_default()
            );
        }
        out.push_str("  }\n}\n");
    }
    out
}

/// `DESIGN.md`: the brand contract in Open Design's nine-section layout,
/// filled with what the CSS tells us and clear placeholders for the rest.
#[must_use]
pub fn render_design_md(system: &DesignSystem) -> String {
    let tokens_of = |kind: TokenKind| -> Vec<&Token> {
        system.tokens.iter().filter(|t| t.kind == kind).collect()
    };
    let mut md = format!(
        "---\nname: {}\nsource: generated from the project's CSS by the Next Agent Design Studio\ntokens: tokens.css\n---\n\n# {} design system\n\n",
        system.name, system.name
    );
    md.push_str("This file is the brand contract the Design Studio designs from. Keep it short and specific; edit anything below.\n\n");

    let colors = tokens_of(TokenKind::Color);
    let has_dark = system.tokens.iter().any(|t| t.dark.is_some());
    let _ = write!(md, "## 1. {}\n\n", DESIGN_MD_SECTIONS[0]);
    let _ = write!(md,
        "- Restrained, functional interface{}.\n- Colour is used for meaning (accent, status), not decoration.\n- _Describe the mood in a sentence: who uses it, and how it should feel._\n\n",
        if has_dark { " with light and dark themes that follow the OS" } else { "" }
    );

    let _ = write!(md, "## 2. {}\n\n", DESIGN_MD_SECTIONS[1]);
    if colors.is_empty() {
        md.push_str("_No colours found._\n\n");
    } else {
        md.push_str(if has_dark {
            "| Token | Light | Dark | Role |\n|---|---|---|---|\n"
        } else {
            "| Token | Value | Role |\n|---|---|---|\n"
        });
        for t in &colors {
            let role = color_role(&t.name);
            if has_dark {
                let _ = writeln!(
                    md,
                    "| `{}` | `{}` | `{}` | {role} |",
                    t.name,
                    t.value,
                    t.dark.as_deref().unwrap_or("—")
                );
            } else {
                let _ = writeln!(md, "| `{}` | `{}` | {role} |", t.name, t.value);
            }
        }
        md.push('\n');
    }

    let _ = write!(md, "## 3. {}\n\n", DESIGN_MD_SECTIONS[2]);
    let font_tokens = tokens_of(TokenKind::Font);
    let fonts: Vec<String> = font_tokens
        .iter()
        .map(|t| format!("`{}` = {}", t.name, t.value))
        .chain(
            system
                .fonts
                .iter()
                .take(3)
                .filter(|f| !font_tokens.iter().any(|t| &t.value == *f))
                .cloned(),
        )
        .collect();
    if fonts.is_empty() {
        md.push_str("- Typeface: system UI stack.\n");
    } else {
        for f in dedup(fonts) {
            let _ = writeln!(md, "- Typeface: {f}");
        }
    }
    let sizes = list_or(&tokens_of(TokenKind::Size), &system.font_sizes);
    if !sizes.is_empty() {
        let _ = writeln!(md, "- Sizes in use: {sizes}");
    }
    md.push_str("- Headings are tight (`margin: 0`); body text inherits the root size.\n\n");

    let _ = write!(md, "## 4. {}\n\n", DESIGN_MD_SECTIONS[3]);
    md.push_str("- Buttons: panel background, 1px border, small radius; the primary button fills with the accent colour.\n- Inputs: full width, panel background, same border and radius as buttons.\n- Focus: a visible 2px outline in the focus colour on every interactive element.\n- _List any other components that have a fixed look._\n\n");

    let _ = write!(md, "## 5. {}\n\n", DESIGN_MD_SECTIONS[4]);
    let spacing = list_or(&tokens_of(TokenKind::Spacing), &system.spacing);
    let _ = write!(md,
        "- Spacing steps: {}\n- Prefer grid and flex layouts with `gap` over margins.\n- Dense, desktop-first panels; content never touches the window edge.\n\n",
        if spacing.is_empty() { "_none found_".to_string() } else { spacing }
    );

    let _ = write!(md, "## 6. {}\n\n", DESIGN_MD_SECTIONS[5]);
    let radii = list_or(&tokens_of(TokenKind::Radius), &system.radii);
    let shadows = list_or(&tokens_of(TokenKind::Shadow), &system.shadows);
    let _ = write!(md,
        "- Radii: {}\n- Shadows: {}\n- Separate surfaces with borders and background steps before reaching for shadows.\n\n",
        if radii.is_empty() { "_none found_".to_string() } else { radii },
        if shadows.is_empty() { "none (flat)".to_string() } else { shadows }
    );

    let _ = write!(md, "## 7. {}\n\n", DESIGN_MD_SECTIONS[6]);
    md.push_str("- Do use the tokens in `tokens.css` (`var(--name)`) instead of raw values.\n- Do keep contrast at WCAG AA or better in both themes.\n- Don't introduce new colours, fonts or radii without adding a token.\n- Don't load web fonts, images or scripts from the network.\n\n");

    let _ = write!(md, "## 8. {}\n\n", DESIGN_MD_SECTIONS[7]);
    md.push_str("- Desktop first; layouts must still work down to 375px wide.\n- Collapse side panels below about 900px.\n\n");

    let _ = write!(md, "## 9. {}\n\n", DESIGN_MD_SECTIONS[8]);
    md.push_str("When designing for this project: link `tokens.css` or inline its `:root` block, use only its variables for colour, type, spacing and radius, support both colour schemes, and make every control keyboard-reachable with a visible focus ring.\n");
    if !system.sources.is_empty() {
        let _ = write!(
            md,
            "\n<!-- Read from: {} -->\n",
            system
                .sources
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    md
}

fn list_or(tokens: &[&Token], fallback: &[String]) -> String {
    if tokens.is_empty() {
        fallback
            .iter()
            .take(8)
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        tokens
            .iter()
            .map(|t| format!("`{}` ({})", t.name, t.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn dedup(items: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}

fn color_role(name: &str) -> &'static str {
    let n = name.trim_start_matches('-').to_ascii_lowercase();
    let roles: &[(&[&str], &str)] = &[
        (&["bg", "background", "canvas"], "Page background"),
        (&["panel", "surface", "card"], "Raised surface"),
        (&["sidebar", "nav"], "Navigation surface"),
        (
            &["border", "line", "divider", "outline"],
            "Borders and dividers",
        ),
        (&["muted", "subtle", "secondary"], "Secondary text"),
        (&["text", "fg", "foreground", "ink"], "Body text"),
        (
            &["accent", "primary", "brand"],
            "Accent, primary actions, links",
        ),
        (&["focus", "ring"], "Focus ring"),
        (&["info"], "Information"),
        (&["warn", "warning", "caution"], "Warning"),
        (
            &["danger", "error", "destructive"],
            "Errors, destructive actions",
        ),
        (&["ok", "success", "positive"], "Success"),
    ];
    roles
        .iter()
        .find(|(keys, _)| {
            keys.iter().any(|k| {
                n == *k || n.starts_with(&format!("{k}-")) || n.ends_with(&format!("-{k}"))
            })
        })
        .map_or("—", |(_, role)| role)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_CSS: &str = ":root {\n  --bg: #fafafa; --panel: #ffffff; --accent: #3b5bdb; --danger: #d6293a;\n  font-family: -apple-system, sans-serif; font-size: 14px;\n}\n@media (prefers-color-scheme: dark) { :root { --bg: #14161a; --accent: #7c93f5; } }\nbutton { border-radius: 6px; padding: 4px 10px; }\n";

    #[test]
    fn extracts_tokens_with_dark_values() {
        let s = extract_from_sources("App", &[("src/app.css".into(), APP_CSS.into())]);
        let bg = s.tokens.iter().find(|t| t.name == "--bg").expect("bg");
        assert_eq!(bg.value, "#fafafa");
        assert_eq!(bg.dark.as_deref(), Some("#14161a"));
        assert_eq!(bg.kind, TokenKind::Color);
        assert!(s
            .tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Color)
            .all(|t| !t.derived));
        // No radius / spacing tokens: derived from literals.
        assert!(s
            .tokens
            .iter()
            .any(|t| t.name == "--radius-1" && t.value == "6px" && t.derived));
        assert!(s
            .tokens
            .iter()
            .any(|t| t.name == "--space-1" && t.value == "4px"));
        assert!(s
            .tokens
            .iter()
            .any(|t| t.name == "--text-1" && t.value == "14px"));
        assert_eq!(s.fonts, vec!["-apple-system, sans-serif"]);
    }

    #[test]
    fn the_root_font_wins_over_frequent_ones() {
        let css = ":root { font-family: Inter, sans-serif; } code, pre, .mono { font-family: ui-monospace, monospace; } .a { font-family: ui-monospace, monospace; } .b { font-family: inherit; }";
        let s = extract_from_sources("X", &[("app.css".into(), css.into())]);
        assert_eq!(
            s.fonts,
            vec!["Inter, sans-serif", "ui-monospace, monospace"]
        );
        let font = s
            .tokens
            .iter()
            .find(|t| t.kind == TokenKind::Font)
            .expect("font");
        assert_eq!(font.value, "Inter, sans-serif");
    }

    #[test]
    fn literal_only_projects_get_a_palette() {
        let css = ".a { color: #111; background: #fff } .b { color: #111; border: 1px solid #ddd }";
        let s = extract_from_sources("Plain", &[("a.css".into(), css.into())]);
        assert_eq!(s.palette[0], ("#111".to_string(), 2));
        let colors: Vec<_> = s
            .tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Color)
            .collect();
        assert_eq!(colors[0].name, "--color-1");
        assert_eq!(colors[0].value, "#111");
    }

    #[test]
    fn global_sheets_win_over_components() {
        let s = extract_from_sources(
            "X",
            &[
                (
                    "src/lib/Card.svelte".into(),
                    ":root { --accent: red; }".into(),
                ),
                ("src/app.css".into(), ":root { --accent: blue; }".into()),
            ],
        );
        assert_eq!(s.tokens[0].value, "blue");
        assert_eq!(s.sources[0], "src/app.css");
    }

    #[test]
    fn component_local_vars_are_not_tokens() {
        let s = extract_from_sources(
            "X",
            &[("a.css".into(), ".card .x { --local: 3px; }".into())],
        );
        assert!(!s.tokens.iter().any(|t| t.name == "--local"));
    }

    #[test]
    fn classify_by_name_and_var_chain() {
        assert_eq!(classify("--radius-md", "6px"), TokenKind::Radius);
        assert_eq!(classify("--space-2", "8px"), TokenKind::Spacing);
        assert_eq!(
            classify("--font-sans", "Inter, sans-serif"),
            TokenKind::Font
        );
        assert_eq!(
            classify("--shadow-lg", "0 1px 2px #0003"),
            TokenKind::Shadow
        );
        let s = extract_from_sources(
            "X",
            &[(
                "a.css".into(),
                ":root { --blue: #00f; --link: var(--blue); }".into(),
            )],
        );
        assert_eq!(s.tokens[1].kind, TokenKind::Color);
    }

    #[test]
    fn renders_tokens_css_and_design_md() {
        let s = extract_from_sources("App", &[("src/app.css".into(), APP_CSS.into())]);
        let tokens = render_tokens_css(&s);
        assert!(tokens.contains("  --accent: #3b5bdb;"));
        assert!(tokens.contains("@media (prefers-color-scheme: dark)"));
        assert!(tokens.contains("    --accent: #7c93f5;"));
        let md = render_design_md(&s);
        for (i, section) in DESIGN_MD_SECTIONS.iter().enumerate() {
            assert!(
                md.contains(&format!("## {}. {section}", i + 1)),
                "{section}"
            );
        }
        assert!(
            md.contains("| `--accent` | `#3b5bdb` | `#7c93f5` | Accent, primary actions, links |")
        );
        // Round trip: tokens.css re-reads to the same values.
        let again = extract_from_sources("App", &[("tokens.css".into(), tokens)]);
        let accent = again
            .tokens
            .iter()
            .find(|t| t.name == "--accent")
            .expect("accent");
        assert_eq!(accent.dark.as_deref(), Some("#7c93f5"));
    }

    #[test]
    fn collects_from_disk_and_skips_noise() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::create_dir_all(root.join("node_modules/x")).expect("nm");
        std::fs::write(root.join("src/app.css"), ":root{--a:#fff}").expect("w");
        std::fs::write(
            root.join("src/A.svelte"),
            "<div/><style>.a{color:red}</style>",
        )
        .expect("w");
        std::fs::write(root.join("node_modules/x/b.css"), ":root{--b:#000}").expect("w");
        std::fs::write(root.join("src/notes.txt"), "--c: 1").expect("w");
        let found = collect_styles(root).expect("collect");
        let names: Vec<_> = found.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, vec!["src/A.svelte", "src/app.css"]);
        assert_eq!(found[0].1, ".a{color:red}");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let outside = tempfile::tempdir().expect("tmp");
        std::fs::write(outside.path().join("secret.css"), ":root{--s:#123}").expect("w");
        let dir = tempfile::tempdir().expect("tmp");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).expect("link");
        std::os::unix::fs::symlink(outside.path().join("secret.css"), dir.path().join("s.css"))
            .expect("link");
        assert_eq!(
            collect_styles(dir.path()).expect("collect"),
            [] as [(std::string::String, std::string::String); 0]
        );
    }
}
