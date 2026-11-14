//! Round-trip and index tests over the real-world-style corpus in
//! `fixtures/logseq-graph` (a complete Logseq graph folder).
#![allow(
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::assert_is_empty
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use next_agent_graph::{BlockNode, Document, Graph, GraphWatcher, Index, RefKind};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/logseq-graph")
}

fn all_md(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("ls") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            all_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("ls") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

fn page(rel: &str) -> (String, Document) {
    let text = fs::read_to_string(corpus_root().join(rel)).expect("read");
    let doc = Document::parse(&text);
    (text, doc)
}

#[test]
fn every_corpus_file_round_trips_byte_for_byte() {
    let mut files = Vec::new();
    all_md(&corpus_root(), &mut files);
    assert!(files.len() >= 40, "corpus has {} files", files.len());
    for path in &files {
        let bytes = fs::read(path).expect("read");
        let text = String::from_utf8(bytes).expect("utf8");
        let doc = Document::parse(&text);
        assert_eq!(
            doc.render(),
            text,
            "parse → render changed {}",
            path.display()
        );

        // What the UI does: read the tree as JSON, send it back, save.
        let nodes: Vec<BlockNode> = doc.blocks.iter().map(BlockNode::from_block).collect();
        let json = serde_json::to_string(&nodes).expect("json");
        let back: Vec<BlockNode> = serde_json::from_str(&json).expect("json back");
        let mut saved = doc.clone();
        saved.replace_blocks(back.iter().map(BlockNode::to_block).collect());
        assert_eq!(
            saved.render(),
            text,
            "editor save changed {}",
            path.display()
        );

        // Re-parsing a rendering of the tree gives the same contents.
        let reparsed = Document::parse(&doc.render());
        assert_eq!(
            reparsed
                .walk()
                .iter()
                .map(|(d, b)| (*d, b.content.clone()))
                .collect::<Vec<_>>(),
            doc.walk()
                .iter()
                .map(|(d, b)| (*d, b.content.clone()))
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn structure_of_selected_pages() {
    let (_, doc) = page("pages/Next Agent.md");
    let props = doc.page_properties();
    assert_eq!(
        props.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
        ["type", "status", "icon"]
    );
    let goals = &doc.blocks[1];
    let task = goals.children[0].children[0].info();
    assert_eq!(task.marker.as_deref(), Some("TODO"));
    assert_eq!(task.priority.as_deref(), Some("A"));
    assert_eq!(task.scheduled.as_deref(), Some("2026-11-14 Sat"));
    let doing = &goals.children[0].children[1];
    assert!(doing.content.contains(":LOGBOOK:\nCLOCK:"));

    let (_, rust) = page("pages/Learn Rust.md");
    assert_eq!(rust.blocks.len(), 2);
    assert_eq!(
        rust.blocks[0].children.len(),
        2,
        "dash inside code is not a block"
    );

    let (_, spaces) = page("pages/Spaces Indented.md");
    assert_eq!(spaces.blocks[0].children.len(), 2);
    assert_eq!(
        spaces.blocks[0].children[0].children[0].content,
        "grandchild"
    );

    let (_, crlf) = page("pages/CRLF Page.md");
    assert_eq!(crlf.blocks[0].content, "First line\nsecond line");

    let (_, fm) = page("pages/Frontmatter List.md");
    let fmp = fm.page_properties();
    assert_eq!(fmp[1].value, "alpha, beta");
    assert_eq!(fmp[2].value, "FL, Front List");

    let (_, first) = page("pages/First Block Properties.md");
    assert!(first.first_block_is_page_properties());
    assert_eq!(first.page_properties()[0].key, "alias");

    let (_, norm) = page("pages/Property Normalisation.md");
    let keys: Vec<_> = norm.blocks[0]
        .info()
        .properties
        .into_iter()
        .map(|p| p.key)
        .collect();
    assert_eq!(keys, ["background-color", "created-at", "my-key", "url"]);

    let (_, custom) = page("pages/Custom ID.md");
    assert_eq!(
        custom.blocks[0].info().uuid().as_deref(),
        Some("65a1b2c3-d4e5-4f60-8172-839405a6b7c8")
    );

    let (_, deep) = page("pages/Deep Nesting.md");
    assert_eq!(deep.walk().iter().map(|(d, _)| *d).max(), Some(6));

    let (_, unclosed) = page("pages/Unclosed Fence.md");
    assert_eq!(
        unclosed.blocks.len(),
        1,
        "an unclosed fence swallows the rest"
    );

    let (_, embeds) = page("pages/Embeds.md");
    assert!(embeds.blocks[0]
        .info()
        .refs
        .iter()
        .any(|r| matches!(r, next_agent_graph::Ref::Page { name, kind: RefKind::Embed } if name == "Dune")));
}

#[test]
fn editing_one_block_leaves_the_rest_byte_identical() {
    let (text, mut doc) = page("pages/Trailing Spaces.md");
    doc.blocks[1].content = "tab edited".into();
    let out = doc.render();
    assert_eq!(out, text.replace("- tab\t\n", "- tab edited\n"));
}

fn indexed_copy() -> (tempfile::TempDir, Graph, Index) {
    let dir = tempfile::tempdir().expect("tmp");
    copy_dir(&corpus_root(), dir.path());
    let graph = Graph::open(dir.path()).expect("graph");
    let mut index = Index::open_in_memory().expect("index");
    let stats = index.rebuild(&graph).expect("rebuild");
    assert!(stats.conflicts.is_empty(), "{:?}", stats.conflicts);
    (dir, graph, index)
}

#[test]
fn index_counts_ignore_hidden_backup_and_assets() {
    let (_dir, graph, index) = indexed_copy();
    let files = graph.files().expect("files");
    assert_eq!(files.len(), 49);
    assert!(!files
        .iter()
        .any(|f| f.contains("archive") || f.contains("bak") || f.contains(".hidden")));
    let (pages, blocks) = index.counts().expect("counts");
    assert_eq!(pages, 49);
    assert!(blocks > 150, "{blocks}");
}

#[test]
fn search_ranks_and_snippets() {
    let (_dir, _graph, index) = indexed_copy();
    let hits = index.search("spice", 10).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, "Dune");
    assert!(
        hits[0].snippet.contains("\u{E000}spice\u{E001}"),
        "{}",
        hits[0].snippet
    );
    // Prefix match on the last word; hidden id:: lines are not searchable.
    assert!(!index.search("mind-kil", 10).expect("prefix").is_empty());
    let by_id = index.search("64f0a1b2", 10).expect("id");
    assert!(!by_id.is_empty(), "((uuid)) refs are text");
    assert!(
        by_id.iter().all(|h| h.page != "Dune"),
        "the id:: line itself is hidden"
    );
    // Diacritics folded, FTS syntax neutralised.
    assert!(!index.search("unicode", 10).expect("fold").is_empty());
    assert!(index.search("\"OR( NEAR", 10).is_ok());
    assert!(index.search("   ", 10).expect("blank").is_empty());
}

#[test]
fn backlinks_follow_aliases_tags_properties_and_embeds() {
    let (_dir, _graph, index) = indexed_copy();
    let links = index.backlinks("Dune").expect("backlinks");
    let pages: Vec<_> = links.iter().map(|b| b.page.as_str()).collect();
    // Journal first (newest), then pages by name.
    assert_eq!(pages[0], "Nov 14th, 2026");
    for expected in ["Embeds", "Frank Herbert", "Nested Links", "Reading List"] {
        assert!(
            pages.contains(&expected),
            "{expected} missing from {pages:?}"
        );
    }
    assert!(!pages.contains(&"Dune"), "self references are excluded");
    assert!(
        !pages.contains(&"Queries"),
        "{{query}} arguments are not references"
    );
    let journal = &links[0];
    assert_eq!(journal.breadcrumb, ["Morning pages"]);
    // Same results through an alias, case-insensitively.
    assert_eq!(
        index.backlinks("dune (NOVEL)").expect("alias").len(),
        links.len()
    );

    // Page-level tags from frontmatter and property blocks.
    let gamma = index.backlinks("alpha").expect("frontmatter tag");
    assert_eq!(gamma[0].page, "Frontmatter List");
    let scifi: Vec<_> = index
        .backlinks("scifi")
        .expect("tag")
        .into_iter()
        .map(|b| b.page)
        .collect();
    assert!(scifi.contains(&"Reading List".to_string()) && scifi.contains(&"Dune".to_string()));

    // Namespaced and multi-word tag pages.
    assert_eq!(
        index.page("Projects/Garden").expect("ns").map(|p| p.file),
        Some("pages/Projects___Garden.md".into())
    );
    assert!(
        index.backlinks("Projects/Garden").expect("ns").is_empty(),
        "only a self link"
    );
    assert_eq!(index.backlinks("multi word tag").expect("mw").len(), 1);
    assert_eq!(
        index.page("a:b").expect("colon").map(|p| p.file),
        Some("pages/a%3Ab.md".into())
    );
}

#[test]
fn unlinked_references_are_plain_mentions() {
    let (_dir, _graph, index) = indexed_copy();
    let unlinked = index
        .unlinked_references("Frank Herbert", 20)
        .expect("unlinked");
    let pages: Vec<_> = unlinked.iter().map(|b| b.page.as_str()).collect();
    assert!(pages.contains(&"Unlinked Mentions"), "{pages:?}");
    assert!(!pages.contains(&"Dune"), "linked mentions are not unlinked");
    assert!(unlinked.iter().all(|b| b.page != "Frank Herbert"));
}

#[test]
fn page_tree_and_journals() {
    let (_dir, graph, index) = indexed_copy();
    let tree = index
        .page_tree("Next Agent")
        .expect("tree")
        .expect("exists");
    assert_eq!(tree.len(), 2);
    assert_eq!(
        tree[1].children[0].children[0].marker.as_deref(),
        Some("TODO")
    );
    assert!(index.page_tree("No Such Page").expect("none").is_none());

    let journals = index.journals(10).expect("journals");
    let names: Vec<_> = journals.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["Nov 14th, 2026", "Nov 13th, 2026", "Jan 1st, 2025"]);
    assert_eq!(journals[0].day, 20_261_114);
    assert_eq!(
        graph.journal_rel_path(next_agent_graph::Date::new(2026, 11, 14).expect("d")),
        "journals/2026_11_14.md"
    );
    assert_eq!(
        index
            .block_by_uuid("64F0A1B2-1234-4abc-8def-0123456789ab")
            .expect("uuid")
            .map(|b| b.0),
        Some("Dune".into())
    );
    let found = index.search_pages("proj", 5).expect("pages");
    assert_eq!(found[0].name, "Projects");
}

#[test]
fn incremental_reindex_and_sync() {
    let (dir, graph, mut index) = indexed_copy();
    let path = dir.path().join("pages/Frank Herbert.md");
    fs::write(&path, "- Wrote [[Dune]] and the word zanzibar.\n").expect("edit");
    assert!(index
        .index_file(&graph, "pages/Frank Herbert.md")
        .expect("reindex"));
    assert_eq!(index.search("zanzibar", 5).expect("s").len(), 1);

    fs::remove_file(&path).expect("rm");
    index
        .index_file(&graph, "pages/Frank Herbert.md")
        .expect("removed");
    assert!(index.search("zanzibar", 5).expect("s").is_empty());
    assert!(index.page("Frank Herbert").expect("p").is_none());

    fs::write(dir.path().join("pages/New One.md"), "- fresh quokka\n").expect("new");
    fs::remove_file(dir.path().join("pages/Book.md")).expect("rm book");
    let stats = index.sync(&graph).expect("sync");
    assert_eq!((stats.indexed, stats.removed), (1, 1));
    assert_eq!(index.search("quokka", 5).expect("s")[0].page, "New One");
    let again = index.sync(&graph).expect("noop");
    assert_eq!((again.indexed, again.removed), (0, 0));
}

#[test]
fn duplicate_page_names_are_reported_not_merged() {
    let dir = tempfile::tempdir().expect("tmp");
    let (graph, _) = Graph::init(dir.path()).expect("init");
    fs::write(dir.path().join("pages/One.md"), "title:: Same\n\n- a\n").expect("w");
    fs::write(dir.path().join("pages/Two.md"), "title:: same\n\n- b\n").expect("w");
    let mut index = Index::open_in_memory().expect("index");
    let stats = index.rebuild(&graph).expect("rebuild");
    assert_eq!(stats.conflicts, ["pages/Two.md"]);
    // When the owner of the name goes away, the skipped file takes over.
    fs::remove_file(dir.path().join("pages/One.md")).expect("rm");
    index.index_file(&graph, "pages/One.md").expect("remove");
    let page = index.page("same").expect("page").expect("re-indexed");
    assert_eq!(page.file, "pages/Two.md");
}

#[test]
fn frontmatter_backlinks_read_as_properties_not_yaml() {
    let (_dir, _graph, index) = indexed_copy();
    let hit = &index.backlinks("alpha").expect("tag")[0];
    assert!(!hit.content.contains("---"), "{}", hit.content);
    assert!(
        hit.content.contains("tags:: alpha, beta"),
        "{}",
        hit.content
    );
}

#[test]
fn index_file_on_disk_survives_reopen() {
    let (dir, graph, _) = indexed_copy();
    let db = dir.path().join("cache/index.sqlite");
    {
        let mut index = Index::open(&db).expect("open");
        index.rebuild(&graph).expect("rebuild");
    }
    let mut index = Index::open(&db).expect("reopen");
    assert_eq!(index.counts().expect("c").0, 49);
    assert_eq!(index.sync(&graph).expect("sync").unchanged, 49);
}

#[test]
fn watcher_reindexes_changed_files_after_debounce() {
    let (dir, graph, index) = indexed_copy();
    let index = Arc::new(Mutex::new(index));
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_cb = Arc::clone(&seen);
    let watcher = GraphWatcher::start(
        graph,
        Arc::clone(&index),
        Duration::from_millis(150),
        Some(Box::new(move |files| {
            seen_cb.lock().expect("lock").extend(files.iter().cloned());
        })),
    )
    .expect("watch");
    // Give FSEvents a moment to start delivering.
    std::thread::sleep(Duration::from_millis(300));
    fs::write(dir.path().join("pages/Watched.md"), "- capybara sighting\n").expect("write");
    fs::write(
        dir.path().join("pages/Dune.md"),
        "- rewritten without spice\n",
    )
    .expect("rewrite");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (watched, spice) = {
            let idx = index.lock().expect("lock");
            (
                idx.search("capybara", 5).expect("s").len(),
                idx.search("spice", 5).expect("s").len(),
            )
        };
        if watched == 1 && spice == 1 {
            break; // "spice" now only in the rewritten Dune block
        }
        assert!(
            Instant::now() < deadline,
            "watcher did not reindex (watched={watched}, spice={spice})"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(watcher);
    assert!(seen
        .lock()
        .expect("lock")
        .iter()
        .any(|f| f == "pages/Watched.md"));
}

#[test]
fn file_names_match_shared_fixture() {
    let cases = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/vault/page-file-names.tsv"),
    )
    .expect("fixture");
    let mut checked = 0;
    for line in cases
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 3, "{line:?}");
        assert_eq!(
            next_agent_graph::layout::page_file_body(cols[0]),
            cols[1],
            "{line}"
        );
        assert_eq!(
            next_agent_graph::title_from_file_body(cols[1]),
            cols[2],
            "{line}"
        );
        checked += 1;
    }
    assert!(checked >= 15);
}
