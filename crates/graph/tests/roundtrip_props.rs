//! Property-based round-trip tests for the Logseq Markdown parser/writer.
#![allow(
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::assert_is_empty
)]

use next_agent_graph::{Block, Document};
use proptest::prelude::*;

/// Fragments that exercise every branch of the parser when glued together.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("- ".to_string()),
        Just("-".to_string()),
        Just("-\t".to_string()),
        Just("\t".to_string()),
        Just("  ".to_string()),
        Just(" ".to_string()),
        Just("\n".to_string()),
        Just("\r\n".to_string()),
        Just("\r".to_string()),
        Just("---".to_string()),
        Just("```".to_string()),
        Just("~~~".to_string()),
        Just("key:: value".to_string()),
        Just("id:: 64f0a1b2-1234-4abc-8def-0123456789ab".to_string()),
        Just(":LOGBOOK:".to_string()),
        Just(":END:".to_string()),
        Just("TODO ".to_string()),
        Just("[#A]".to_string()),
        Just("SCHEDULED: <2026-11-14 Sat>".to_string()),
        Just("[[Page]]".to_string()),
        Just("#tag".to_string()),
        Just("#[[a b]]".to_string()),
        Just("((64f0a1b2-1234-4abc-8def-0123456789ab))".to_string()),
        Just("{{embed [[x]]}}".to_string()),
        Just("# ".to_string()),
        "[a-zA-Z0-9 .,:;!?()\\[\\]{}#*_`>|-]{0,12}".prop_map(String::from),
        "\\PC{0,4}".prop_map(String::from),
    ]
}

fn document_text() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..80).prop_map(|parts| parts.concat())
}

/// Block text the writer can always represent: no line that would itself
/// parse as a bullet or a fence, no carriage returns, no trailing blank line.
fn content_line() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 .,:;!?()\\[\\]#*_>|]{0,20}"
        .prop_map(String::from)
        .prop_filter("not a bullet", |l| {
            let t = l.trim_start();
            !(t == "-" || t.starts_with("- ") || t.starts_with("-\t"))
        })
}

fn content() -> impl Strategy<Value = String> {
    prop::collection::vec(content_line(), 1..4).prop_map(|mut lines| {
        while lines.len() > 1 && lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    })
}

fn tree() -> impl Strategy<Value = Vec<Block>> {
    let leaf = content().prop_map(Block::new);
    let node = leaf.prop_recursive(4, 40, 4, |inner| {
        (content(), prop::collection::vec(inner, 0..4))
            .prop_map(|(c, kids)| Block::new(c).with_children(kids))
    });
    prop::collection::vec(node, 0..6)
}

fn shape(blocks: &[Block], depth: usize, out: &mut Vec<(usize, String)>) {
    for b in blocks {
        out.push((depth, b.content.clone()));
        shape(&b.children, depth + 1, out);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Any text at all: parse → render is the identity.
    #[test]
    fn parse_render_is_identity(text in document_text()) {
        prop_assert_eq!(Document::parse(&text).render(), text);
    }

    /// A fresh tree renders to text that parses back to the same tree, and
    /// that text then round-trips byte for byte.
    #[test]
    fn rendered_trees_parse_back(blocks in tree()) {
        let mut expected = Vec::new();
        shape(&blocks, 0, &mut expected);
        let text = Document::new(blocks).render();
        let parsed = Document::parse(&text);
        let mut got = Vec::new();
        shape(&parsed.blocks, 0, &mut got);
        prop_assert_eq!(got, expected);
        prop_assert_eq!(parsed.render(), text);
    }

    /// Editing one block of a parsed file only changes that block's lines.
    #[test]
    fn edits_are_local(blocks in tree(), pick in any::<prop::sample::Index>(), new in content()) {
        let text = Document::new(blocks).render();
        let mut doc = Document::parse(&text);
        let total = doc.walk().len();
        prop_assume!(total > 0);
        let target = pick.index(total);
        let mut flat: Vec<(usize, String)> = Vec::new();
        shape(&doc.blocks, 0, &mut flat);
        flat[target].1.clone_from(&new);

        fn edit(blocks: &mut [Block], target: usize, at: &mut usize, new: &str) {
            for b in blocks {
                if *at == target {
                    b.content = new.to_string();
                }
                *at += 1;
                edit(&mut b.children, target, at, new);
            }
        }
        let mut at = 0;
        edit(&mut doc.blocks, target, &mut at, &new);
        let out = doc.render();
        let mut got = Vec::new();
        shape(&Document::parse(&out).blocks, 0, &mut got);
        prop_assert_eq!(got, flat);
        // Every untouched block keeps its exact bytes.
        let pristine = doc.walk().iter().filter(|(_, b)| b.is_pristine()).count();
        prop_assert!(pristine >= total - 1);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(3000))]

    /// Deriving facts never panics (char boundaries, unclosed brackets,
    /// macros, fences) and every page ref is non-empty text from the input.
    #[test]
    fn analyze_never_panics(text in document_text()) {
        let info = next_agent_graph::inline::analyze(&text);
        for r in info.refs {
            if let next_agent_graph::Ref::Page { name, .. } = r {
                prop_assert!(!name.trim().is_empty());
            }
        }
    }

    /// Any title maps to a single safe file name inside `pages/`.
    #[test]
    fn file_names_are_single_safe_components(title in "\\PC{0,40}") {
        let body = next_agent_graph::layout::page_file_body(&title);
        prop_assert!(!body.contains('/') && !body.contains('\\'));
        prop_assert!(!body.contains([':', '*', '?', '"', '<', '>', '|', '#']));
        prop_assert!(!body.starts_with('.'));
        prop_assert!(body != ".." && body != ".");
    }

    /// Titles without empty namespace parts survive title → file → title.
    #[test]
    fn titles_round_trip_through_file_names(
        parts in prop::collection::vec("[^/\\p{C}]{1,12}", 1..4)
    ) {
        let title = parts.join("/");
        prop_assume!(!title.ends_with('.') && !parts.iter().any(|p| p.trim().is_empty()));
        prop_assume!(!["CON", "PRN", "AUX", "NUL"].contains(&title.as_str()));
        let body = next_agent_graph::layout::page_file_body(&title);
        prop_assert_eq!(next_agent_graph::title_from_file_body(&body), title);
    }
}
