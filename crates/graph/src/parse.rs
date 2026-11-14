// Format follows Logseq (AGPL-3.0, © Logseq contributors): Markdown file
// graphs as read by deps/graph-parser/src/logseq/graph_parser/{mldoc.cljc,
// block.cljs, extract.cljc} @ c67b8b5 (tag 0.10.9). Re-implemented in Rust.
//! Lossless parser and writer for Logseq Markdown pages.
//!
//! A page is an optional *preamble* (YAML frontmatter, `key:: value` page
//! properties, any text before the first bullet) followed by a tree of
//! `- ` blocks. Each parsed block remembers the exact bytes it came from, so
//! [`Document::render`] reproduces an unchanged file byte for byte. Only
//! blocks whose content or depth changed are re-rendered, in Logseq's own
//! style (tab nesting, continuation lines indented two spaces past the
//! bullet's indentation).
//!
//! Anything the parser does not understand stays inside a block's content
//! (or the preamble) as opaque text and is written back unchanged.

use crate::inline::{self, BlockInfo, Property};

/// One `- ` block and its children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The block text as Logseq stores it in `:block/content`: the first line
    /// after `- `, then continuation lines with the indentation removed.
    /// Property lines, `SCHEDULED:` lines and drawers are part of it.
    pub content: String,
    pub children: Vec<Block>,
    origin: Option<Origin>,
}

/// Where a parsed block came from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin {
    /// Exact source bytes of this block's own lines (children excluded).
    raw: String,
    content: String,
    depth: usize,
    indent: String,
}

impl Block {
    /// A new block with no source text (it will be rendered fresh).
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            children: Vec::new(),
            origin: None,
        }
    }

    #[must_use]
    pub fn with_children(mut self, children: Vec<Block>) -> Self {
        self.children = children;
        self
    }

    /// Properties, task marker, refs and other facts derived from `content`.
    #[must_use]
    pub fn info(&self) -> BlockInfo {
        inline::analyze(&self.content)
    }

    /// True when this block will be written back from its source bytes.
    #[must_use]
    pub fn is_pristine(&self) -> bool {
        self.origin
            .as_ref()
            .is_some_and(|o| o.content == self.content)
    }
}

/// A parsed page file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Everything before the first block, verbatim.
    pub preamble: String,
    pub blocks: Vec<Block>,
    newline: &'static str,
    indent_unit: String,
    final_newline: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            preamble: String::new(),
            blocks: Vec::new(),
            newline: "\n",
            indent_unit: "\t".to_string(),
            final_newline: true,
        }
    }
}

struct Line<'a> {
    /// Line text without its terminator.
    body: &'a str,
    /// Line text with its terminator (`\n`, `\r\n` or nothing at EOF).
    full: &'a str,
}

fn split_lines(source: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut rest = source;
    while !rest.is_empty() {
        let (full, next) = match rest.find('\n') {
            Some(at) => rest.split_at(at + 1),
            None => (rest, ""),
        };
        let body = full
            .strip_suffix('\n')
            .map_or(full, |b| b.strip_suffix('\r').unwrap_or(b));
        out.push(Line { body, full });
        rest = next;
    }
    out
}

/// `Some((indent, text))` when the line is a `- ` bullet.
fn bullet(body: &str) -> Option<(&str, &str)> {
    let ws = body
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(body.len());
    let (indent, rest) = body.split_at(ws);
    if rest == "-" {
        Some((indent, ""))
    } else if rest.starts_with("- ") || rest.starts_with("-\t") {
        Some((indent, &rest[2..]))
    } else {
        None
    }
}

fn width(indent: &str) -> usize {
    indent.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Remove a block's indentation from one of its continuation lines.
fn strip_continuation<'a>(body: &'a str, indent: &str) -> &'a str {
    match body.strip_prefix(indent) {
        Some(rest) => rest
            .strip_prefix("  ")
            .or_else(|| rest.strip_prefix('\t'))
            .or_else(|| rest.strip_prefix(' '))
            .unwrap_or(rest),
        None if body.trim().is_empty() => "",
        None => body.trim_start(),
    }
}

/// Tracks fenced code (```` ``` ```` / `~~~`) so bullets inside code are text.
#[derive(Default)]
struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    fn feed(&mut self, line: &str) {
        let t = line.trim_start();
        let Some(ch) = t.chars().next().filter(|c| *c == '`' || *c == '~') else {
            return;
        };
        let run = t.chars().take_while(|c| *c == ch).count();
        if run < 3 {
            return;
        }
        match self.open {
            None => self.open = Some((ch, run)),
            Some((open_ch, open_run))
                if open_ch == ch && run >= open_run && t[run..].trim().is_empty() =>
            {
                self.open = None;
            }
            Some(_) => {}
        }
    }

    fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// Length in lines of a YAML frontmatter block at the top, if any.
fn frontmatter_len(lines: &[Line<'_>]) -> usize {
    if lines.first().map(|l| l.body.trim_end()) != Some("---") {
        return 0;
    }
    lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.body.trim_end() == "---")
        .map_or(0, |(i, _)| i + 1)
}

struct Flat {
    depth: usize,
    block: Block,
}

impl Document {
    /// Parse a Markdown page. Never fails: unknown text stays opaque.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let lines = split_lines(source);
        let newline = if lines.first().is_some_and(|l| l.full.ends_with("\r\n")) {
            "\r\n"
        } else {
            "\n"
        };

        // Preamble: frontmatter, then anything up to the first bullet.
        let mut i = frontmatter_len(&lines);
        let mut fence = Fence::default();
        while i < lines.len() {
            if !fence.is_open() && bullet(lines[i].body).is_some() {
                break;
            }
            fence.feed(lines[i].body);
            i += 1;
        }
        let preamble: String = lines[..i].iter().map(|l| l.full).collect();

        let mut flat: Vec<Flat> = Vec::new();
        let mut stack: Vec<usize> = Vec::new(); // bullet widths of open ancestors
        let mut indent_unit: Option<String> = None;
        let mut parent_indents: Vec<String> = Vec::new();
        while i < lines.len() {
            let Some((indent, first)) = bullet(lines[i].body) else {
                unreachable!("loop only stops on bullets");
            };
            let w = width(indent);
            while stack.last().is_some_and(|top| *top >= w) {
                stack.pop();
                parent_indents.pop();
            }
            let depth = stack.len();
            if indent_unit.is_none() && depth == 1 {
                if let Some(rest) = indent.strip_prefix(parent_indents[0].as_str()) {
                    if !rest.is_empty() {
                        indent_unit = Some(rest.to_string());
                    }
                }
            }
            stack.push(w);
            parent_indents.push(indent.to_string());

            let start = i;
            let mut content: Vec<&str> = vec![first];
            let mut fence = Fence::default();
            fence.feed(first);
            i += 1;
            while i < lines.len() {
                let line = &lines[i];
                if !fence.is_open() && bullet(line.body).is_some() {
                    break;
                }
                let text = strip_continuation(line.body, indent);
                fence.feed(text);
                content.push(text);
                i += 1;
            }
            while content.len() > 1 && content.last().is_some_and(|l| l.trim().is_empty()) {
                content.pop();
            }
            let content = content.join("\n");
            let raw: String = lines[start..i].iter().map(|l| l.full).collect();
            flat.push(Flat {
                depth,
                block: Block {
                    content: content.clone(),
                    children: Vec::new(),
                    origin: Some(Origin {
                        raw,
                        content,
                        depth,
                        indent: indent.to_string(),
                    }),
                },
            });
        }

        Self {
            preamble,
            blocks: build_tree(flat),
            newline,
            indent_unit: indent_unit.unwrap_or_else(|| "\t".to_string()),
            final_newline: source.is_empty() || source.ends_with('\n'),
        }
    }

    /// A new, empty page that uses Logseq's default style.
    #[must_use]
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            ..Self::default()
        }
    }

    /// Write the page back to text. Unchanged blocks are copied from their
    /// source bytes; changed or new blocks are rendered in Logseq's style.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.preamble.clone();
        let mut last_fresh = false;
        for block in &self.blocks {
            self.render_block(block, 0, &mut out, &mut last_fresh);
        }
        if last_fresh && !self.final_newline {
            if let Some(stripped) = out.strip_suffix(self.newline) {
                out.truncate(stripped.len());
            }
        }
        out
    }

    fn render_block(&self, block: &Block, depth: usize, out: &mut String, last_fresh: &mut bool) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(self.newline);
        }
        match &block.origin {
            Some(o) if o.content == block.content && o.depth == depth => {
                out.push_str(&o.raw);
                *last_fresh = false;
            }
            origin => {
                let indent = match origin {
                    Some(o) if o.depth == depth => o.indent.clone(),
                    _ => self.indent_unit.repeat(depth),
                };
                let mut lines = block.content.split('\n');
                let first = lines.next().unwrap_or("");
                out.push_str(&indent);
                out.push('-');
                if !first.is_empty() {
                    out.push(' ');
                    out.push_str(first);
                }
                out.push_str(self.newline);
                for line in lines {
                    if !line.is_empty() {
                        out.push_str(&indent);
                        out.push_str("  ");
                        out.push_str(line);
                    }
                    out.push_str(self.newline);
                }
                *last_fresh = true;
            }
        }
        for child in &block.children {
            self.render_block(child, depth + 1, out, last_fresh);
        }
    }

    /// Every block in document order with its depth (0 = top level).
    #[must_use]
    pub fn walk(&self) -> Vec<(usize, &Block)> {
        flatten(&self.blocks)
    }

    /// Page properties: YAML frontmatter, `key:: value` lines before the
    /// first bullet, or — when there are none — a first block made only of
    /// property lines (Logseq's "pre-block").
    #[must_use]
    pub fn page_properties(&self) -> Vec<Property> {
        let mut props = crate::frontmatter::parse(&self.preamble);
        let body_start = crate::frontmatter::body_offset(&self.preamble);
        for line in self.preamble[body_start..].lines() {
            if let Some(p) = inline::property_line(line.trim_end_matches('\r')) {
                props.push(p);
            }
        }
        if props.is_empty() {
            if let Some(first) = self.blocks.first() {
                if is_properties_only(&first.content) {
                    props = first.info().properties;
                }
            }
        }
        props
    }

    /// True when the first block holds the page properties.
    #[must_use]
    pub fn first_block_is_page_properties(&self) -> bool {
        self.blocks
            .first()
            .is_some_and(|b| is_properties_only(&b.content))
            && crate::frontmatter::parse(&self.preamble).is_empty()
            && !self
                .preamble
                .lines()
                .any(|l| inline::property_line(l).is_some())
    }

    /// Replace the block tree with `blocks` (for example from the editor),
    /// keeping the source bytes of every block that is unchanged. Blocks are
    /// matched by `(depth, content)` along document order, so inserting,
    /// deleting or editing one block leaves the others byte-identical.
    pub fn replace_blocks(&mut self, mut blocks: Vec<Block>) {
        let mut assigned: Vec<Option<Origin>> = {
            let old = flatten(&self.blocks);
            let new = flatten(&blocks);
            let mut assigned = vec![None; new.len()];
            for (o, n) in lcs_matches(&old, &new) {
                assigned[n].clone_from(&old[o].1.origin);
            }
            assigned
        };
        let mut cursor = 0;
        assign_origins(&mut blocks, &mut assigned, &mut cursor);
        self.blocks = blocks;
    }
}

fn is_properties_only(content: &str) -> bool {
    let mut any = false;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if inline::property_line(line).is_none() {
            return false;
        }
        any = true;
    }
    any
}

fn build_tree(flat: Vec<Flat>) -> Vec<Block> {
    // Depth only ever grows by one from one block to the next (see parse).
    let mut roots: Vec<Block> = Vec::new();
    let mut path: Vec<Block> = Vec::new();
    for Flat { depth, block } in flat {
        while path.len() > depth {
            let done = path.pop().expect("non-empty");
            match path.last_mut() {
                Some(parent) => parent.children.push(done),
                None => roots.push(done),
            }
        }
        path.push(block);
    }
    while let Some(done) = path.pop() {
        match path.last_mut() {
            Some(parent) => parent.children.push(done),
            None => roots.push(done),
        }
    }
    roots
}

/// Blocks in document order with their depth.
fn flatten(blocks: &[Block]) -> Vec<(usize, &Block)> {
    fn go<'a>(blocks: &'a [Block], depth: usize, out: &mut Vec<(usize, &'a Block)>) {
        for b in blocks {
            out.push((depth, b));
            go(&b.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    go(blocks, 0, &mut out);
    out
}

/// Give each block (in document order) its matched source bytes, if any.
fn assign_origins(blocks: &mut [Block], assigned: &mut [Option<Origin>], cursor: &mut usize) {
    for b in blocks {
        b.origin = assigned[*cursor].take();
        *cursor += 1;
        assign_origins(&mut b.children, assigned, cursor);
    }
}

/// Longest common subsequence of `(depth, content)` keys. Falls back to
/// position-by-position matching for very large pages.
/// Only pristine old blocks can match (their bytes are worth keeping).
fn lcs_matches(old: &[(usize, &Block)], new: &[(usize, &Block)]) -> Vec<(usize, usize)> {
    let eq = |o: &(usize, &Block), n: &(usize, &Block)| {
        o.0 == n.0 && o.1.is_pristine() && o.1.content == n.1.content
    };
    let (n, m) = (old.len(), new.len());
    if n.saturating_mul(m) > 4_000_000 {
        return (0..n.min(m))
            .filter(|&i| eq(&old[i], &new[i]))
            .map(|i| (i, i))
            .collect();
    }
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if eq(&old[i], &new[j]) {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n && j < m {
        if eq(&old[i], &new[j]) {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_blocks_with_tabs_and_continuations() {
        let src = "- a\n  more a\n\t- b\n\t  more b\n\t\t- c\n- d\n";
        let doc = Document::parse(src);
        assert_eq!(doc.render(), src);
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.blocks[0].content, "a\nmore a");
        assert_eq!(doc.blocks[0].children[0].content, "b\nmore b");
        assert_eq!(doc.blocks[0].children[0].children[0].content, "c");
        assert_eq!(doc.blocks[1].content, "d");
    }

    #[test]
    fn spaces_indentation() {
        let src = "- a\n  - b\n    - c\n  - d\n";
        let doc = Document::parse(src);
        assert_eq!(doc.blocks[0].children.len(), 2);
        assert_eq!(doc.blocks[0].children[0].children[0].content, "c");
        assert_eq!(doc.indent_unit, "  ");
    }

    #[test]
    fn bullets_inside_code_fences_are_text() {
        let src = "- code\n  ```md\n  - not a block\n  ```\n- next\n";
        let doc = Document::parse(src);
        assert_eq!(doc.blocks.len(), 2);
        assert!(doc.blocks[0].content.contains("- not a block"));
    }

    #[test]
    fn preamble_holds_frontmatter_and_page_properties() {
        let src = "---\ntitle: X\n---\nalias:: y\n\n- block\n";
        let doc = Document::parse(src);
        assert_eq!(doc.preamble, "---\ntitle: X\n---\nalias:: y\n\n");
        let props = doc.page_properties();
        assert_eq!(props.len(), 2);
        assert_eq!(doc.render(), src);
    }

    #[test]
    fn edited_block_is_rerendered_others_kept() {
        let src = "- a  \n\t- b\n- c\n";
        let mut doc = Document::parse(src);
        doc.blocks[0].children[0].content = "B\nline".into();
        assert_eq!(doc.render(), "- a  \n\t- B\n\t  line\n- c\n");
    }

    #[test]
    fn crlf_and_missing_final_newline() {
        let src = "- a\r\n\t- b";
        let mut doc = Document::parse(src);
        assert_eq!(doc.render(), src);
        doc.blocks[0].children[0].content = "c".into();
        assert_eq!(doc.render(), "- a\r\n\t- c");
        doc.blocks.push(Block::new("d"));
        assert_eq!(doc.render(), "- a\r\n\t- c\r\n- d");
    }

    #[test]
    fn replace_blocks_keeps_untouched_bytes() {
        let src = "- one   \n- two\t\n- three  \n";
        let mut doc = Document::parse(src);
        doc.replace_blocks(vec![
            Block::new("zero"),
            Block::new("one   "),
            Block::new("two\t"),
            Block::new("three  "),
        ]);
        assert_eq!(doc.render(), "- zero\n- one   \n- two\t\n- three  \n");
    }

    #[test]
    fn empty_and_dash_only_bullets() {
        let src = "-\n- \n-\tx\n";
        let doc = Document::parse(src);
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(doc.blocks[2].content, "x");
        assert_eq!(doc.render(), src);
    }

    #[test]
    fn horizontal_rule_is_not_a_bullet() {
        let doc = Document::parse("---\n- a\n");
        assert_eq!(doc.preamble, "---\n");
        assert_eq!(doc.blocks.len(), 1);
    }
}
