//! A small, forgiving CSS reader: enough structure to pull design tokens out
//! of real project stylesheets (custom properties, colours, type, spacing,
//! radii, shadows) without a full CSS parser.
//!
//! It understands comments, quoted strings, nested blocks (`@media`,
//! `@supports`, CSS nesting) and declarations, and records for each
//! declaration the chain of selectors / at-rules it sits in, so callers can
//! tell a `:root` token from a dark-mode override.

/// One `property: value` with the block chain it was declared in
/// (outermost first, e.g. `["@media (prefers-color-scheme: dark)", ":root"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub context: Vec<String>,
}

impl Declaration {
    /// True when any enclosing block selects a dark theme.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        self.context.iter().any(|c| is_dark_context(c))
    }

    /// True when the innermost selector is a document root (`:root`, `html`,
    /// `:host`, `body`) or a theme variant of one (`:root.dark`,
    /// `[data-theme=dark]`), i.e. where design tokens normally live.
    #[must_use]
    pub fn is_root_level(&self) -> bool {
        self.context
            .iter()
            .rev()
            .find(|c| !c.starts_with('@'))
            .is_none_or(|sel| {
                sel.split(',').any(|s| {
                    let s = s.trim().to_ascii_lowercase();
                    s.starts_with(":root")
                        || s.starts_with("html")
                        || s.starts_with(":host")
                        || s.starts_with("body")
                        || s.starts_with("[data-theme")
                        || s.starts_with(".dark")
                        || s.starts_with(".light")
                        || s.starts_with(".theme")
                })
            })
    }
}

/// A selector or at-rule prelude that turns on a dark theme.
fn is_dark_context(prelude: &str) -> bool {
    let p = prelude.to_ascii_lowercase().replace(' ', "");
    p.contains("prefers-color-scheme:dark")
        || p.contains(".dark")
        || p.contains("theme=dark")
        || p.contains("theme=\"dark\"")
        || p.contains("theme='dark'")
}

/// Remove `/* … */` comments (unterminated comments run to the end).
#[must_use]
pub fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The contents of every `<style>` element in a Svelte / Vue / HTML file.
#[must_use]
pub fn style_blocks(markup: &str) -> Vec<String> {
    let lower = markup.to_ascii_lowercase();
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(open) = lower[from..].find("<style") {
        let open = from + open;
        let Some(head_end) = lower[open..].find('>') else {
            break;
        };
        let body_start = open + head_end + 1;
        let Some(close) = lower[body_start..].find("</style") else {
            break;
        };
        blocks.push(markup[body_start..body_start + close].to_string());
        from = body_start + close + 7;
    }
    blocks
}

/// Every declaration in `css`, in source order.
#[must_use]
pub fn declarations(css: &str) -> Vec<Declaration> {
    let css = strip_comments(css);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    let mut parens = 0usize;
    for ch in css.chars() {
        if let Some(q) = quote {
            buf.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                buf.push(ch);
            }
            '(' => {
                parens += 1;
                buf.push(ch);
            }
            ')' => {
                parens = parens.saturating_sub(1);
                buf.push(ch);
            }
            ';' if parens == 0 => {
                push_declaration(&mut out, &buf, &stack);
                buf.clear();
            }
            '{' if parens == 0 => {
                stack.push(collapse_ws(buf.trim()));
                buf.clear();
            }
            '}' if parens == 0 => {
                push_declaration(&mut out, &buf, &stack);
                buf.clear();
                stack.pop();
            }
            _ => buf.push(ch),
        }
    }
    out
}

fn push_declaration(out: &mut Vec<Declaration>, raw: &str, stack: &[String]) {
    if stack.is_empty() {
        return; // `@import …;` and stray text outside blocks
    }
    let Some((property, value)) = raw.split_once(':') else {
        return;
    };
    let property = property.trim();
    let value = value.trim().trim_end_matches("!important").trim();
    let valid_name = !property.is_empty()
        && property
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid_name || value.is_empty() {
        return;
    }
    let property = if property.starts_with("--") {
        property.to_string()
    } else {
        property.to_ascii_lowercase()
    };
    out.push(Declaration {
        property,
        value: collapse_ws(value),
        context: stack.to_vec(),
    });
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Colour literals inside a value: `#rgb[a]`, `#rrggbb[aa]` and the
/// functional forms (`rgb()`, `hsl()`, `oklch()`, …), lower-cased.
#[must_use]
pub fn colors_in(value: &str) -> Vec<String> {
    const FUNCS: &[&str] = &[
        "rgba(", "rgb(", "hsla(", "hsl(", "oklch(", "oklab(", "lch(", "lab(", "hwb(",
    ];
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let hex = bytes[i + 1..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            let boundary = bytes
                .get(i + 1 + hex)
                .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'-');
            if matches!(hex, 3 | 4 | 6 | 8) && boundary {
                found.push(lower[i..=i + hex].to_string());
            }
            i += 1 + hex;
            continue;
        }
        let at_word_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'-';
        if at_word_start {
            if let Some(f) = FUNCS.iter().find(|f| lower[i..].starts_with(**f)) {
                if let Some(close) = lower[i..].find(')') {
                    found.push(collapse_ws(&lower[i..=i + close]));
                    i += close + 1;
                    continue;
                }
                i += f.len();
                continue;
            }
        }
        i += lower[i..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// True when the whole value is a single colour (literal or keyword).
#[must_use]
pub fn is_color_value(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    if matches!(
        v.as_str(),
        "transparent" | "currentcolor" | "white" | "black"
    ) {
        return true;
    }
    let found = colors_in(&v);
    found.len() == 1 && found[0] == v
}

/// Length-like parts of a value (`4px`, `0.5rem`, `1.25em`, `0`), skipping
/// keywords, `var()` and percentages.
#[must_use]
pub fn lengths_in(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|part| is_length(part))
        .map(str::to_string)
        .collect()
}

/// `12px`, `1.5rem`, `0.75em`, `2ch` and friends (a non-zero number + unit).
#[must_use]
pub fn is_length(part: &str) -> bool {
    const UNITS: &[&str] = &["px", "rem", "em", "ch", "vh", "vw", "pt"];
    UNITS.iter().any(|unit| {
        part.strip_suffix(unit).is_some_and(|num| {
            !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit() || c == '.')
                && num.chars().any(|c| c.is_ascii_digit() && c != '0')
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_declarations_with_context() {
        let css = "/* c { } */ :root { --bg: #fff; --text: #111 }\n\
                   @media (prefers-color-scheme: dark) { :root { --bg: #000; } }\n\
                   .card { padding: 8px 12px; font-family: \"Inter; x\", sans-serif; }";
        let d = declarations(css);
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].property, "--bg");
        assert!(d[0].is_root_level() && !d[0].is_dark());
        assert_eq!(d[1].value, "#111");
        assert!(d[2].is_dark());
        assert_eq!(d[2].context.len(), 2);
        assert!(!d[3].is_root_level());
        assert_eq!(d[4].value, "\"Inter; x\", sans-serif");
    }

    #[test]
    fn parens_protect_semicolons_and_braces() {
        let d = declarations(".a { background: url(data:image/svg+xml;utf8,{x}); color: red }");
        assert_eq!(d.len(), 2);
        assert!(d[0].value.starts_with("url("));
    }

    #[test]
    fn style_blocks_in_markup() {
        let src = "<script>let x</script><div/><style lang=\"postcss\">.a{color:red}</style><STYLE>.b{}</STYLE>";
        assert_eq!(style_blocks(src), vec![".a{color:red}", ".b{}"]);
    }

    #[test]
    fn finds_colors() {
        assert_eq!(
            colors_in("1px solid #DCDEE3, rgb(0 0 0 / 0.2) #abc"),
            vec!["#dcdee3", "rgb(0 0 0 / 0.2)", "#abc"]
        );
        assert_eq!(colors_in("#ab #abcde #zzz"), Vec::<String>::new());
        assert_eq!(colors_in("var(--x)"), Vec::<String>::new());
        assert_eq!(colors_in("\"→\" #abc"), vec!["#abc"]);
        assert!(is_color_value("#3B5BDB"));
        assert!(is_color_value("hsl(220 50% 40%)"));
        assert!(!is_color_value("1px solid #fff"));
    }

    #[test]
    fn finds_lengths() {
        assert_eq!(lengths_in("4px 0.5rem 0 auto 50%"), vec!["4px", "0.5rem"]);
        assert!(!is_length("0px"));
        assert!(is_length("14px"));
    }

    #[test]
    fn unterminated_comment_is_dropped() {
        assert_eq!(strip_comments("a/* b"), "a");
    }
}
