//! Notes engine for the UI: one open Logseq graph, its SQLite index (kept in
//! the app data folder, never in the user's vault) and a file watcher.
//! The Tauri commands at the bottom are thin wrappers over [`NotesService`].
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use next_agent_graph::{
    parse_date, Backlink, BlockNode, Date, Document, FileKind, Graph, GraphData, GraphWatcher,
    Index, IndexStats, JournalEntry, PageListItem, PageView, QueryEnv, SearchHit,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::State;

/// Log one index pass: file count and time only.
fn log_indexed(stats: &IndexStats, started: Instant) {
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(files = stats.indexed, duration_ms, "graph_indexed");
}

/// Called with the graph-relative files the watcher re-indexed.
pub type ChangeListener = Arc<dyn Fn(Vec<String>) + Send + Sync>;

const DEFAULT_SEARCH_LIMIT: usize = 50;
const DEFAULT_JOURNALS: usize = 30;
/// Upper bound on search results per request.
const MAX_SEARCH_LIMIT: usize = 500;
/// Upper bound on journal entries per request.
const MAX_JOURNALS_LIMIT: usize = 1000;
const UNLINKED_LIMIT: usize = 100;
const QUERY_LIMIT: usize = 300;
const GRAPH_EDGES: usize = 20_000;
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

/// The block a `((uuid))` points at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockRefDto {
    pub page: String,
    pub content: String,
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

/// Stable file name for a graph's index (hash of its canonical path).
fn cache_name(root: &Path) -> String {
    let hash = next_agent_core::hash::sha256_hex(root.to_string_lossy().as_bytes());
    format!("graph-{}.sqlite", &hash[..32])
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

    /// Open a graph folder, bring its index up to date and start watching
    /// it. Replaces any graph opened before. Missing `pages/`, `journals/`,
    /// `assets/` and `logseq/config.edn` are created only when the folder
    /// already looks like a graph, is empty, or `create` is set, so opening
    /// the wrong folder never litters it.
    pub fn open(&self, path: &str, create: bool) -> Result<GraphInfo, String> {
        let root = PathBuf::from(path.trim());
        if !root.is_dir() {
            return Err(format!("not a folder: {}", root.display()));
        }
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if !create && !Graph::looks_like_graph(&root) {
            return Err(format!(
                "{} is not a Logseq graph (no logseq/, pages/ or journals/); open it with create to make one",
                root.display()
            ));
        }
        let (graph, created_config) = Graph::init(&root).map_err(|e| e.to_string())?;
        let mut index =
            Index::open(&self.cache_dir.join(cache_name(&root))).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let stats = index.sync(&graph).map_err(|e| e.to_string())?;
        log_indexed(&stats, started);
        let (pages, blocks) = index.counts().map_err(|e| e.to_string())?;
        let index = Arc::new(Mutex::new(index));
        let listener = lock(&self.listener)?.clone();
        let on_change = listener.map(|l| {
            Box::new(move |files: &[String]| l(files.to_vec())) as next_agent_graph::watch::OnChange
        });
        // A graph still works without a watcher (e.g. on network drives).
        let watcher = GraphWatcher::start(graph.clone(), Arc::clone(&index), DEBOUNCE, on_change)
            .inspect_err(|_| {
                tracing::warn!(error = "file watcher could not start", "watcher_error");
            })
            .ok();
        let previous = lock(&self.open)?.replace(OpenGraph {
            graph,
            index,
            _watcher: watcher,
        });
        // Stopping the old watcher may wait for a sync; don't hold the lock.
        drop(previous);
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
        let mut open = lock(&self.open)?;
        let open = open.as_mut().ok_or("open a graph first")?;
        let mut index = lock(&open.index)?;
        // Follow config.edn edits (a stat unless it changed). `sync` rebuilds
        // once if the settings differ from what the index was built with,
        // whether the watcher or this call notices first.
        if open.graph.reload_config() {
            index.sync(&open.graph).map_err(|e| e.to_string())?;
        }
        f(&open.graph, &mut index)
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<SearchHit>, String> {
        self.with(|_, index| {
            index
                .search(
                    query,
                    limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT),
                )
                .map_err(|e| e.to_string())
        })
    }

    /// Graph-relative file for a page name: the indexed file, or where a
    /// new page (or journal day) with that name would be written.
    fn file_for(graph: &Graph, index: &Index, name: &str) -> Result<(String, bool), String> {
        if let Some(page) = index.page(name).map_err(|e| e.to_string())? {
            return Ok((page.file, true));
        }
        let rel = graph.rel_path_for_name(name);
        let exists = graph.resolve(&rel).map_err(|e| e.to_string())?.is_file();
        Ok((rel, exists))
    }

    fn view(graph: &Graph, rel: &str, doc: &Document) -> PageView {
        let properties = doc.page_properties();
        PageView {
            name: graph.page_name_from(rel, &properties),
            file: Some(rel.to_string()),
            journal_day: match graph.classify(rel) {
                Some(FileKind::Journal(d)) => Some(d.as_int()),
                _ => None,
            },
            properties,
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
                .journals(limit.unwrap_or(DEFAULT_JOURNALS).min(MAX_JOURNALS_LIMIT))
                .map_err(|e| e.to_string())
        })
    }

    /// Save a page's block tree. Blocks the editor did not change keep their
    /// exact bytes; the preamble (frontmatter, page properties) is kept. A
    /// new page whose name cannot be carried by its file name gets a
    /// `title::` property, as Logseq does. Writes are atomic and re-indexed
    /// at once.
    pub fn save_page(&self, name: &str, tree: Vec<BlockNode>) -> Result<PageView, String> {
        self.with(|graph, index| {
            let (rel, exists) = Self::file_for(graph, index, name)?;
            let mut doc = if exists {
                graph.read(&rel)
            } else {
                // Rejects empty names and control characters (a newline
                // would inject property lines via `title::`).
                graph.new_page_document(name)
            }
            .map_err(|e| e.to_string())?;
            doc.replace_blocks(tree.iter().map(BlockNode::to_block).collect());
            let text = doc.render();
            graph.write_text(&rel, &text).map_err(|e| e.to_string())?;
            index.index_file(graph, &rel).map_err(|e| e.to_string())?;
            Ok(Self::view(graph, &rel, &Document::parse(&text)))
        })
    }

    /// Every page, for the all-pages list.
    pub fn pages(&self) -> Result<Vec<PageListItem>, String> {
        self.with(|_, index| index.all_pages().map_err(|e| e.to_string()))
    }

    /// Page nodes and links for the graph view.
    pub fn graph_data(&self) -> Result<GraphData, String> {
        self.with(|_, index| index.graph_data(GRAPH_EDGES).map_err(|e| e.to_string()))
    }

    /// The block that carries `id:: uuid`, if the graph has one.
    pub fn block(&self, uuid: &str) -> Result<Option<BlockRefDto>, String> {
        self.with(|_, index| {
            Ok(index
                .block_by_uuid(&uuid.to_lowercase())
                .map_err(|e| e.to_string())?
                .map(|(page, content)| BlockRefDto { page, content }))
        })
    }

    /// The page name of a journal day (`yyyyMMdd`) in the graph's format.
    pub fn journal_name(&self, day: i64) -> Result<String, String> {
        let date = Date::from_int(day).ok_or("not a date")?;
        self.with(|graph, _| Ok(graph.journal_title(date)))
    }

    /// Run a simple `{{query …}}`; `today` is `yyyyMMdd` in the user's clock.
    pub fn query(&self, text: &str, today: i64) -> Result<Vec<Backlink>, String> {
        let today = Date::from_int(today).ok_or("today is not a date")?;
        self.with(|graph, index| {
            let format = graph.config().journal_title_format.clone();
            let env = QueryEnv {
                today,
                parse_journal: &|s| parse_date(&format, s),
            };
            index
                .query(text, &env, QUERY_LIMIT)
                .map_err(|e| e.to_string())
        })
    }

    /// Drop the index and rebuild it from the files.
    pub fn rebuild(&self) -> Result<IndexStats, String> {
        self.with(|graph, index| {
            let started = Instant::now();
            let stats = index.rebuild(graph).map_err(|e| e.to_string())?;
            log_indexed(&stats, started);
            Ok(stats)
        })
    }
}

// ------------------------------------------------------------- commands --

type Notes<'a> = State<'a, Arc<NotesService>>;

#[tauri::command]
pub fn graph_open(notes: Notes, path: String, create: Option<bool>) -> Result<GraphInfo, String> {
    notes.open(&path, create.unwrap_or(false))
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

// --- notes-ui ---
#[tauri::command]
pub fn graph_pages(notes: Notes) -> Result<Vec<PageListItem>, String> {
    notes.pages()
}

#[tauri::command]
pub fn graph_graph(notes: Notes) -> Result<GraphData, String> {
    notes.graph_data()
}

#[tauri::command]
pub fn graph_block(notes: Notes, uuid: String) -> Result<Option<BlockRefDto>, String> {
    notes.block(&uuid)
}

#[tauri::command]
pub fn graph_journal_name(notes: Notes, day: i64) -> Result<String, String> {
    notes.journal_name(day)
}

#[tauri::command]
pub fn graph_query(notes: Notes, text: String, today: i64) -> Result<Vec<Backlink>, String> {
    notes.query(&text, today)
}
// --- end notes-ui ---

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
        let info = notes
            .open(&graph.display().to_string(), false)
            .expect("open");
        assert_eq!(info.pages, 49);
        assert!(!info.created_config);
        (dir, notes, graph)
    }

    #[test]
    fn commands_need_an_open_graph() {
        let notes = NotesService::new(std::env::temp_dir().join("na-notes-none"));
        assert_eq!(notes.search("x", None).unwrap_err(), "open a graph first");
        assert!(notes.open("/definitely/not/here", true).is_err());
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
        assert!(notes.save_page("x\nalias:: injected", Vec::new()).is_err());
        // Names that look like paths stay one file inside pages/.
        let sneaky = notes
            .save_page("../../etc/passwd", Vec::new())
            .expect("escaped");
        assert_eq!(
            sneaky.file.as_deref(),
            Some("pages/%2E.___..___etc___passwd.md")
        );
        assert!(graph.join("pages/%2E.___..___etc___passwd.md").is_file());
    }

    #[test]
    fn queries_pages_and_graph_data() {
        let (_dir, notes, _) = opened();
        let block = |c: &str| BlockNode {
            content: c.into(),
            ..BlockNode::default()
        };
        notes
            .save_page(
                "Query Lab",
                vec![
                    block("TODO [#A] Water [[Tomatoes]]"),
                    block("DOING Weed [[Tomatoes]]\nowner:: [[Sam]]"),
                    block("DONE Plant [[Tomatoes]]"),
                    block("TODO Pay rent"),
                    block("TODO 100% done_ish"),
                ],
            )
            .expect("save");
        let q = |text: &str| -> Vec<String> {
            notes
                .query(text, 20_261_114)
                .expect(text)
                .into_iter()
                .map(|b| b.content.lines().next().unwrap_or("").to_string())
                .collect()
        };
        assert_eq!(
            q("{{query (and [[Tomatoes]] (page \"Query Lab\") (task TODO DOING))}}"),
            ["TODO [#A] Water [[Tomatoes]]", "DOING Weed [[Tomatoes]]"]
        );
        assert_eq!(
            q("(and [[tomatoes]] (page \"query lab\") (not (task DONE)))").len(),
            2
        );
        assert_eq!(
            q("(and (page \"Query Lab\") (priority A))"),
            ["TODO [#A] Water [[Tomatoes]]"]
        );
        assert_eq!(
            q("(and (page \"query lab\") (or (task DONE) \"rent\"))").len(),
            2
        );
        assert_eq!(
            q("(and (page \"Query Lab\") (property owner Sam))"),
            ["DOING Weed [[Tomatoes]]"]
        );
        assert_eq!(q("(and (page \"Query Lab\") (property owner))").len(), 1);
        assert_eq!(q("\"100%\""), ["TODO 100% done_ish"]);
        assert_eq!(q("\"100_\"").len(), 0, "LIKE wildcards are escaped");
        // The fixture's journal days: Nov 13 and Nov 14, 2026.
        let between = notes
            .query("(between [[Nov 13th, 2026]] today)", 20_261_114)
            .expect("between");
        assert_ne!(between.len(), 0);
        assert!(between.iter().all(|b| b.journal_day.is_some()));
        assert!(notes.query("(between -1d today)", 20_261_114).is_ok());
        assert!(notes.query("(nope)", 20_261_114).is_err());
        assert!(notes.query("[[x]]", 5).is_err());
        // Quote injection stays data.
        assert_eq!(q("\"'); DROP TABLE blocks; --\"").len(), 0);
        assert!(notes.search("water", None).is_ok());
        assert_eq!(
            notes.journal_name(20_261_115).expect("name"),
            "Nov 15th, 2026"
        );
        assert!(notes.journal_name(20_261_399).is_err());
        let referenced = notes.page("Block Refs").expect("page");
        let target = referenced
            .blocks
            .iter()
            .find_map(|b| b.uuid.clone())
            .or_else(|| {
                notes.page("Dune").expect("dune").blocks[0].children[0]
                    .uuid
                    .clone()
            })
            .expect("a block with an id");
        let hit = notes.block(&target).expect("block").expect("found");
        assert!(!hit.page.is_empty() && !hit.content.is_empty());
        assert!(notes
            .block("00000000-0000-4000-8000-000000000000")
            .expect("block")
            .is_none());

        let pages = notes.pages().expect("pages");
        let lab = pages.iter().find(|p| p.name == "Query Lab").expect("lab");
        assert_eq!(lab.blocks, 5);
        let tomatoes = notes.graph_data().expect("graph");
        assert!(tomatoes
            .edges
            .iter()
            .any(|e| e.source == "query lab" && e.target == "tomatoes"));
        let t = tomatoes
            .nodes
            .iter()
            .find(|n| n.id == "tomatoes")
            .expect("node");
        assert!(t.missing && t.degree >= 1);
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
            .open(&graph.display().to_string(), false)
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
    fn config_edits_rename_journals_without_a_manual_rebuild() {
        let (_dir, notes, graph) = opened();
        let config = graph.join("logseq/config.edn");
        let text = fs::read_to_string(&config).expect("config");
        fs::write(&config, text.replace("\"MMM do, yyyy\"", "\"yyyy-MM-dd\"")).expect("edit");
        let page = notes.page("2026-11-14").expect("page");
        assert_eq!(page.file.as_deref(), Some("journals/2026_11_14.md"));
        assert_eq!(notes.journals(Some(1)).expect("j")[0].name, "2026-11-14");
    }

    #[test]
    fn opening_an_empty_folder_creates_the_layout() {
        let dir = tempfile::tempdir().expect("tmp");
        let notes = NotesService::new(dir.path().join("cache"));
        let vault = dir.path().join("vault");
        fs::create_dir_all(&vault).expect("mkdir");
        let info = notes
            .open(&vault.display().to_string(), false)
            .expect("empty folder is fine");
        assert!(info.created_config);
        let project = dir.path().join("project");
        fs::create_dir_all(&project).expect("mkdir");
        fs::write(project.join("README.md"), "hi").expect("file");
        assert!(notes.open(&project.display().to_string(), false).is_err());
        assert!(
            !project.join("logseq").exists(),
            "nothing written into a non-graph folder"
        );
        assert!(
            notes
                .open(&project.display().to_string(), true)
                .expect("create")
                .created_config
        );
        assert!(vault.join("logseq/config.edn").is_file());
        assert!(vault.join("pages").is_dir() && vault.join("journals").is_dir());
        assert_eq!(info.pages, 0);
    }
}
