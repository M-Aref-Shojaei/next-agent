//! Notes engine for the UI: one open Logseq graph, its SQLite index (kept in
//! the app data folder, never in the user's vault) and a file watcher.
//! The Tauri commands at the bottom are thin wrappers over [`NotesService`].
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use next_agent_graph::layout::needs_title_property;
use next_agent_graph::{
    parse_date, Backlink, BlockNode, Document, FileKind, Graph, GraphWatcher, Index, IndexStats,
    JournalEntry, PageView, SearchHit,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tauri::State;

/// Called with the graph-relative files the watcher re-indexed.
pub type ChangeListener = Arc<dyn Fn(Vec<String>) + Send + Sync>;

const DEFAULT_SEARCH_LIMIT: usize = 50;
const DEFAULT_JOURNALS: usize = 30;
const UNLINKED_LIMIT: usize = 100;
const DEBOUNCE: Duration = Duration::from_millis(300);

/// What `graph_open` returns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphInfo {
    pub root: String,
    pub pages: i64,
    pub blocks: i64,
    /// Whether a `logseq/config.edn` had to be created.
    pub created_config: bool,
    pub stats: IndexStats,
}

/// Linked and unlinked references of a page.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BacklinksDto {
    pub linked: Vec<Backlink>,
    pub unlinked: Vec<Backlink>,
}

struct OpenGraph {
    graph: Graph,
    index: Arc<Mutex<Index>>,
    _watcher: Option<GraphWatcher>,
}

/// UI-independent notes service.
pub struct NotesService {
    cache_dir: PathBuf,
    open: Mutex<Option<OpenGraph>>,
    listener: Mutex<Option<ChangeListener>>,
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    m.lock().map_err(|_| "notes state is poisoned".to_string())
}

/// Stable file name for a graph's index (FNV-1a of its canonical path).
fn cache_name(root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.to_string_lossy().as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("graph-{hash:016x}.sqlite")
}

impl NotesService {
    /// `cache_dir` holds one index database per opened graph.
    #[must_use]
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            open: Mutex::new(None),
            listener: Mutex::new(None),
        }
    }

    /// Notify `listener` after the watcher re-indexes files.
    pub fn set_listener(&self, listener: ChangeListener) {
        if let Ok(mut l) = self.listener.lock() {
            *l = Some(listener);
        }
    }

    /// Open a graph folder (creating `pages/`, `journals/`, `assets/` and a
    /// minimal `logseq/config.edn` when missing), bring its index up to date
    /// and start watching it. Replaces any graph opened before.
    pub fn open(&self, path: &str) -> Result<GraphInfo, String> {
        let root = PathBuf::from(path.trim());
        if !root.is_dir() {
            return Err(format!("not a folder: {}", root.display()));
        }
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let (graph, created_config) = Graph::init(&root).map_err(|e| e.to_string())?;
        let mut index =
            Index::open(&self.cache_dir.join(cache_name(&root))).map_err(|e| e.to_string())?;
        let stats = index.sync(&graph).map_err(|e| e.to_string())?;
        let (pages, blocks) = index.counts().map_err(|e| e.to_string())?;
        let index = Arc::new(Mutex::new(index));
        let listener = lock(&self.listener)?.clone();
        let on_change = listener.map(|l| {
            Box::new(move |files: &[String]| l(files.to_vec())) as next_agent_graph::watch::OnChange
        });
        // A graph still works without a watcher (e.g. on network drives).
        let watcher =
            GraphWatcher::start(graph.clone(), Arc::clone(&index), DEBOUNCE, on_change).ok();
        *lock(&self.open)? = Some(OpenGraph {
            graph,
            index,
            _watcher: watcher,
        });
        Ok(GraphInfo {
            root: root.display().to_string(),
            pages,
            blocks,
            created_config,
            stats,
        })
    }

    fn with<R>(
        &self,
        f: impl FnOnce(&Graph, &mut Index) -> Result<R, String>,
    ) -> Result<R, String> {
        let open = lock(&self.open)?;
        let open = open.as_ref().ok_or("open a graph first")?;
        let mut index = lock(&open.index)?;
        f(&open.graph, &mut index)
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<SearchHit>, String> {
        self.with(|_, index| {
            index
                .search(query, limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(500))
                .map_err(|e| e.to_string())
        })
    }

    /// Graph-relative file for a page name: the indexed file, or where a
    /// new page (or journal day) with that name would be written.
    fn file_for(graph: &Graph, index: &Index, name: &str) -> Result<(String, bool), String> {
        if let Some(page) = index.page(name).map_err(|e| e.to_string())? {
            return Ok((page.file, true));
        }
        let journal = parse_date(&graph.config().journal_title_format, name.trim());
        let rel = match journal {
            Some(date) => graph.journal_rel_path(date),
            None => graph.page_rel_path(name.trim()),
        };
        let exists = graph.resolve(&rel).map_err(|e| e.to_string())?.is_file();
        Ok((rel, exists))
    }

    fn view(graph: &Graph, rel: &str, doc: &Document) -> PageView {
        PageView {
            name: graph.page_name(rel, doc),
            file: Some(rel.to_string()),
            journal_day: match graph.classify(rel) {
                Some(FileKind::Journal(d)) => Some(d.as_int()),
                _ => None,
            },
            properties: doc.page_properties(),
            preamble: doc.preamble.clone(),
            blocks: doc.blocks.iter().map(BlockNode::from_block).collect(),
        }
    }

    /// A page's block tree, read from its file (the source of truth). A
    /// page that is only referenced comes back with no file and no blocks.
    pub fn page(&self, name: &str) -> Result<PageView, String> {
        if name.trim().is_empty() {
            return Err("page name is empty".into());
        }
        self.with(|graph, index| {
            let (rel, exists) = Self::file_for(graph, index, name)?;
            if !exists {
                return Ok(PageView {
                    name: name.trim().to_string(),
                    ..PageView::default()
                });
            }
            let doc = graph.read(&rel).map_err(|e| e.to_string())?;
            Ok(Self::view(graph, &rel, &doc))
        })
    }

    pub fn backlinks(&self, name: &str) -> Result<BacklinksDto, String> {
        self.with(|_, index| {
            Ok(BacklinksDto {
                linked: index.backlinks(name).map_err(|e| e.to_string())?,
                unlinked: index
                    .unlinked_references(name, UNLINKED_LIMIT)
                    .map_err(|e| e.to_string())?,
            })
        })
    }

    pub fn journals(&self, limit: Option<usize>) -> Result<Vec<JournalEntry>, String> {
        self.with(|_, index| {
            index
                .journals(limit.unwrap_or(DEFAULT_JOURNALS).min(1000))
                .map_err(|e| e.to_string())
        })
    }

    /// Save a page's block tree. Blocks the editor did not change keep their
    /// exact bytes; the preamble (frontmatter, page properties) is kept. A
    /// new page whose name cannot be carried by its file name gets a
    /// `title::` property, as Logseq does. Writes are atomic and re-indexed
    /// at once.
    pub fn save_page(&self, name: &str, tree: Vec<BlockNode>) -> Result<PageView, String> {
        if name.trim().is_empty() {
            return Err("page name is empty".into());
        }
        self.with(|graph, index| {
            let (rel, exists) = Self::file_for(graph, index, name)?;
            let mut doc = if exists {
                graph.read(&rel).map_err(|e| e.to_string())?
            } else {
                let mut doc = Document::default();
                if needs_title_property(name.trim()) && graph.classify(&rel) == Some(FileKind::Page)
                {
                    doc.preamble = format!("title:: {}\n\n", name.trim());
                }
                doc
            };
            doc.replace_blocks(tree.iter().map(BlockNode::to_block).collect());
            graph.write(&rel, &doc).map_err(|e| e.to_string())?;
            index.index_file(graph, &rel).map_err(|e| e.to_string())?;
            let saved = graph.read(&rel).map_err(|e| e.to_string())?;
            Ok(Self::view(graph, &rel, &saved))
        })
    }

    /// Drop the index and rebuild it from the files.
    pub fn rebuild(&self) -> Result<IndexStats, String> {
        self.with(|graph, index| index.rebuild(graph).map_err(|e| e.to_string()))
    }
}

// ------------------------------------------------------------- commands --

type Notes<'a> = State<'a, Arc<NotesService>>;

#[tauri::command]
pub fn graph_open(notes: Notes, path: String) -> Result<GraphInfo, String> {
    notes.open(&path)
}

#[tauri::command]
pub fn graph_search(
    notes: Notes,
    q: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    notes.search(&q, limit)
}

#[tauri::command]
pub fn graph_page(notes: Notes, name: String) -> Result<PageView, String> {
    notes.page(&name)
}

#[tauri::command]
pub fn graph_backlinks(notes: Notes, name: String) -> Result<BacklinksDto, String> {
    notes.backlinks(&name)
}

#[tauri::command]
pub fn graph_journals(notes: Notes, limit: Option<usize>) -> Result<Vec<JournalEntry>, String> {
    notes.journals(limit)
}

#[tauri::command]
pub fn graph_save_page(
    notes: Notes,
    name: String,
    tree: Vec<BlockNode>,
) -> Result<PageView, String> {
    notes.save_page(&name, tree)
}

#[tauri::command]
pub fn graph_rebuild(notes: Notes) -> Result<IndexStats, String> {
    notes.rebuild()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

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

    fn opened() -> (tempfile::TempDir, NotesService, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let graph = dir.path().join("graph");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/logseq-graph"),
            &graph,
        );
        let notes = NotesService::new(dir.path().join("cache"));
        let info = notes.open(&graph.display().to_string()).expect("open");
        assert_eq!(info.pages, 49);
        assert!(!info.created_config);
        (dir, notes, graph)
    }

    #[test]
    fn commands_need_an_open_graph() {
        let notes = NotesService::new(std::env::temp_dir().join("na-notes-none"));
        assert_eq!(notes.search("x", None).unwrap_err(), "open a graph first");
        assert!(notes.open("/definitely/not/here").is_err());
    }

    #[test]
    fn search_page_backlinks_and_journals() {
        let (_dir, notes, _) = opened();
        assert_eq!(notes.search("spice", None).expect("search")[0].page, "Dune");

        let page = notes.page("dune").expect("page");
        assert_eq!(page.name, "Dune");
        assert_eq!(page.file.as_deref(), Some("pages/Dune.md"));
        assert_eq!(page.blocks[0].children.len(), 2);
        assert!(page.blocks[0].children[0].uuid.is_some());

        let refs = notes.backlinks("Dune").expect("backlinks");
        assert!(refs.linked.iter().any(|b| b.page == "Embeds"));
        assert!(refs.unlinked.iter().any(|b| b.page == "Unlinked Mentions"));

        let journals = notes.journals(Some(2)).expect("journals");
        assert_eq!(journals.len(), 2);
        assert_eq!(journals[0].day, 20_261_114);
        let day = notes.page(&journals[0].name).expect("journal page");
        assert_eq!(day.journal_day, Some(20_261_114));

        let missing = notes.page("Never Written").expect("ref-only page");
        assert_eq!((missing.file, missing.blocks.len()), (None, 0));
        assert_eq!(
            notes.rebuild().expect("rebuild").conflicts,
            Vec::<String>::new()
        );
    }

    #[test]
    fn saving_an_unchanged_tree_keeps_the_file_byte_identical() {
        let (_dir, notes, graph) = opened();
        for name in [
            "Next Agent",
            "CRLF Page",
            "Trailing Spaces",
            "Frontmatter List",
            "Nov 14th, 2026",
        ] {
            let page = notes.page(name).expect("page");
            let file = graph.join(page.file.as_deref().expect("file"));
            let before = fs::read(&file).expect("read");
            // Through JSON, as the UI sends it.
            let tree: Vec<BlockNode> =
                serde_json::from_str(&serde_json::to_string(&page.blocks).expect("json"))
                    .expect("parse");
            notes.save_page(name, tree).expect("save");
            assert_eq!(fs::read(&file).expect("read"), before, "{name}");
        }
    }

    #[test]
    fn saving_edits_and_new_pages() {
        let (_dir, notes, graph) = opened();
        let mut page = notes.page("Garden Redesign").expect("page");
        page.blocks[1].content = "DONE Order seeds".into();
        page.blocks.push(BlockNode {
            content: "Plant [[Tomatoes]]\nwith a second line".into(),
            ..BlockNode::default()
        });
        let saved = notes
            .save_page("Garden Redesign", page.blocks)
            .expect("save");
        assert_eq!(saved.blocks[1].marker.as_deref(), Some("DONE"));
        let text = fs::read_to_string(graph.join("pages/Garden Redesign.md")).expect("read");
        assert!(text.starts_with("- DONE Measure the beds\n- DONE Order seeds\n- WAITING"));
        assert!(text.ends_with("- Plant [[Tomatoes]]\n  with a second line\n"));
        assert_eq!(notes.backlinks("Tomatoes").expect("bl").linked.len(), 1);

        let made = notes
            .save_page(
                "Next Agent/Notes/Ideas: v2",
                vec![BlockNode {
                    content: "first idea".into(),
                    children: vec![BlockNode {
                        content: "child".into(),
                        ..BlockNode::default()
                    }],
                    ..BlockNode::default()
                }],
            )
            .expect("new page");
        assert_eq!(
            made.file.as_deref(),
            Some("pages/Next Agent___Notes___Ideas%3A v2.md")
        );
        assert_eq!(made.name, "Next Agent/Notes/Ideas: v2");
        let text = fs::read_to_string(graph.join("pages/Next Agent___Notes___Ideas%3A v2.md"))
            .expect("read");
        assert_eq!(
            text,
            "title:: Next Agent/Notes/Ideas: v2\n\n- first idea\n\t- child\n"
        );
        assert_eq!(
            notes.search("first idea", None).expect("s")[0].page,
            made.name
        );

        let day = notes
            .save_page(
                "Nov 15th, 2026",
                vec![BlockNode {
                    content: "tomorrow".into(),
                    ..BlockNode::default()
                }],
            )
            .expect("journal");
        assert_eq!(day.file.as_deref(), Some("journals/2026_11_15.md"));
        assert_eq!(
            notes.journals(Some(1)).expect("j")[0].name,
            "Nov 15th, 2026"
        );
        assert!(notes.save_page("  ", Vec::new()).is_err());
    }

    #[test]
    fn watcher_picks_up_outside_edits_and_notifies() {
        let (_dir, notes, graph) = opened();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&seen);
        notes.set_listener(Arc::new(move |files| {
            sink.lock().expect("lock").extend(files);
        }));
        notes
            .open(&graph.display().to_string())
            .expect("reopen with listener");
        std::thread::sleep(Duration::from_millis(300));
        fs::write(
            graph.join("pages/From Logseq.md"),
            "- written by another app: axolotl\n",
        )
        .expect("write");
        let deadline = Instant::now() + Duration::from_secs(10);
        while notes.search("axolotl", None).expect("search").is_empty() {
            assert!(
                Instant::now() < deadline,
                "watcher did not index the new file"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !seen
            .lock()
            .expect("lock")
            .iter()
            .any(|f| f == "pages/From Logseq.md")
        {
            assert!(Instant::now() < deadline, "listener not called");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn opening_an_empty_folder_creates_the_layout() {
        let dir = tempfile::tempdir().expect("tmp");
        let notes = NotesService::new(dir.path().join("cache"));
        let vault = dir.path().join("vault");
        fs::create_dir_all(&vault).expect("mkdir");
        let info = notes.open(&vault.display().to_string()).expect("open");
        assert!(info.created_config);
        assert!(vault.join("logseq/config.edn").is_file());
        assert!(vault.join("pages").is_dir() && vault.join("journals").is_dir());
        assert_eq!(info.pages, 0);
    }
}
