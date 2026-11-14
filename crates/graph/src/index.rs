// Format follows Logseq (AGPL-3.0, © Logseq contributors): the page / block /
// ref model of deps/graph-parser/src/logseq/graph_parser/extract.cljc
// @ c67b8b5 (tag 0.10.9), stored in SQLite instead of DataScript.
//! Rebuildable SQLite index of a graph: pages, blocks (parent, order,
//! content, properties), refs, aliases and FTS5 full-text search.
//!
//! The Markdown files stay the source of truth; this database is a cache
//! that [`Index::rebuild`] can recreate at any time. It is the only file in
//! this crate that talks to SQLite (see `scripts/check-boundaries.mjs`).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::inline::{self, Property, Ref};
use crate::layout::{FileKind, Graph, GraphError};
use crate::model::BlockNode;
use crate::parse::{Block, Document};

/// Bump when the schema changes; older caches are dropped and rebuilt.
const SCHEMA_VERSION: &str = "1";

/// Snippet highlight markers (Unicode private use), so the UI can split the
/// snippet safely instead of rendering HTML.
pub const MATCH_START: char = '\u{E000}';
pub const MATCH_END: char = '\u{E001}';

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE files(path TEXT PRIMARY KEY, mtime_ns INTEGER NOT NULL, size INTEGER NOT NULL);
CREATE TABLE pages(
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  name_lc TEXT NOT NULL UNIQUE,
  file TEXT NOT NULL UNIQUE,
  journal_day INTEGER,
  properties TEXT NOT NULL
);
CREATE INDEX pages_journal ON pages(journal_day);
CREATE TABLE blocks(
  id INTEGER PRIMARY KEY,
  page_id INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
  parent_id INTEGER,
  ord INTEGER NOT NULL,
  depth INTEGER NOT NULL,
  uuid TEXT,
  content TEXT NOT NULL,
  marker TEXT,
  priority TEXT,
  scheduled TEXT,
  deadline TEXT,
  properties TEXT NOT NULL
);
CREATE INDEX blocks_page ON blocks(page_id, ord);
CREATE INDEX blocks_uuid ON blocks(uuid);
CREATE TABLE refs(
  src_block INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
  src_page INTEGER NOT NULL,
  dst_page_lc TEXT,
  dst_uuid TEXT,
  kind TEXT NOT NULL
);
CREATE INDEX refs_dst_page ON refs(dst_page_lc);
CREATE INDEX refs_dst_uuid ON refs(dst_uuid);
CREATE TABLE aliases(
  alias_lc TEXT NOT NULL,
  page_id INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE
);
CREATE INDEX aliases_lc ON aliases(alias_lc);
CREATE VIRTUAL TABLE blocks_fts USING fts5(text, tokenize = 'unicode61 remove_diacritics 2');
";

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index database: {0}")]
    Db(String),
    #[error(transparent)]
    Graph(#[from] GraphError),
}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        Self::Graph(GraphError::Io(e))
    }
}

type Result<T> = std::result::Result<T, IndexError>;

/// What a rebuild or sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub indexed: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Files skipped because another file already has the same page name.
    pub conflicts: Vec<String>,
}

/// A full-text search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page: String,
    pub block_id: i64,
    pub uuid: Option<String>,
    pub content: String,
    /// Excerpt with matches between [`MATCH_START`] and [`MATCH_END`].
    pub snippet: String,
    /// Higher is better (negated bm25).
    pub score: f64,
}

/// A block that references (or mentions) a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub page: String,
    pub journal_day: Option<i64>,
    pub block_id: i64,
    pub uuid: Option<String>,
    pub content: String,
    /// First lines of the ancestor blocks, outermost first.
    pub breadcrumb: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub name: String,
    pub file: String,
    pub day: i64,
}

/// A page row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRow {
    pub id: i64,
    pub name: String,
    pub file: String,
    pub journal_day: Option<i64>,
}

/// SQLite index of one graph.
pub struct Index {
    conn: Connection,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index").finish_non_exhaustive()
    }
}

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

/// FTS5 query from user text: every word must match, the last as a prefix.
fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .collect();
    let last = words.len().checked_sub(1)?;
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == last {
                    format!("\"{w}\"*")
                } else {
                    format!("\"{w}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

impl Index {
    /// Open (or create) an index database file.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// An in-memory index (tests, one-off queries).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
                r.get(0)
            })
            .optional()
            .unwrap_or(None);
        if version.as_deref() != Some(SCHEMA_VERSION) {
            conn.execute_batch(
                "DROP TABLE IF EXISTS blocks_fts; DROP TABLE IF EXISTS refs;
                 DROP TABLE IF EXISTS aliases; DROP TABLE IF EXISTS blocks;
                 DROP TABLE IF EXISTS pages; DROP TABLE IF EXISTS files;
                 DROP TABLE IF EXISTS meta;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema', ?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Ok(Self { conn })
    }

    /// Drop everything and index every file of the graph.
    ///
    /// # Errors
    /// SQLite or I/O failures.
    pub fn rebuild(&mut self, graph: &Graph) -> Result<IndexStats> {
        {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "DELETE FROM blocks_fts; DELETE FROM refs; DELETE FROM aliases;
                 DELETE FROM blocks; DELETE FROM pages; DELETE FROM files;",
            )?;
            tx.commit()?;
        }
        self.sync(graph)
    }

    /// Incremental update: re-index files whose size or mtime changed and
    /// drop files that disappeared.
    ///
    /// # Errors
    /// SQLite or I/O failures.
    pub fn sync(&mut self, graph: &Graph) -> Result<IndexStats> {
        let mut stats = IndexStats::default();
        let known: HashMap<String, (i64, i64)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT path, mtime_ns, size FROM files")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        let files = graph.files()?;
        let on_disk: HashSet<&str> = files.iter().map(String::as_str).collect();
        let tx = self.conn.transaction()?;
        for path in known.keys() {
            if !on_disk.contains(path.as_str()) {
                remove_file_tx(&tx, path)?;
                stats.removed += 1;
            }
        }
        for rel in &files {
            let meta = std::fs::metadata(graph.resolve(rel)?)?;
            let stamp = (
                mtime_ns(&meta),
                i64::try_from(meta.len()).unwrap_or(i64::MAX),
            );
            if known.get(rel) == Some(&stamp) {
                stats.unchanged += 1;
                continue;
            }
            match index_file_tx(&tx, graph, rel)? {
                FileResult::Indexed => stats.indexed += 1,
                FileResult::Conflict => stats.conflicts.push(rel.clone()),
                FileResult::Removed => stats.removed += 1,
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    /// Re-index one graph-relative file, or remove it when it no longer
    /// exists (or is not a page/journal file).
    ///
    /// # Errors
    /// SQLite or I/O failures.
    pub fn index_file(&mut self, graph: &Graph, rel: &str) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let result = index_file_tx(&tx, graph, rel)?;
        tx.commit()?;
        Ok(result == FileResult::Indexed)
    }

    /// Forget one file.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn remove_file(&mut self, rel: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        remove_file_tx(&tx, rel)?;
        tx.commit()?;
        Ok(())
    }

    /// Number of indexed pages and blocks.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn counts(&self) -> Result<(i64, i64)> {
        Ok((
            self.conn
                .query_row("SELECT count(*) FROM pages", [], |r| r.get(0))?,
            self.conn
                .query_row("SELECT count(*) FROM blocks", [], |r| r.get(0))?,
        ))
    }

    /// Find a page by name or alias (case-insensitive).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn page(&self, name: &str) -> Result<Option<PageRow>> {
        let lc = name.trim().to_lowercase();
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, file, journal_day FROM pages WHERE name_lc = ?1
                 UNION ALL
                 SELECT p.id, p.name, p.file, p.journal_day FROM aliases a
                   JOIN pages p ON p.id = a.page_id WHERE a.alias_lc = ?1
                 LIMIT 1",
                [&lc],
                |r| {
                    Ok(PageRow {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        file: r.get(2)?,
                        journal_day: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Lowercased names that mean this page: the name itself, the page it
    /// resolves to and all of that page's aliases.
    fn target_names(&self, name: &str) -> Result<(Vec<String>, Option<i64>)> {
        let mut names = vec![name.trim().to_lowercase()];
        let page = self.page(name)?;
        if let Some(p) = &page {
            names.push(p.name.to_lowercase());
            let mut stmt = self
                .conn
                .prepare("SELECT alias_lc FROM aliases WHERE page_id = ?1")?;
            for alias in stmt.query_map([p.id], |r| r.get::<_, String>(0))? {
                names.push(alias?);
            }
        }
        names.sort();
        names.dedup();
        Ok((names, page.map(|p| p.id)))
    }

    /// Full-text search over block text, best match first.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.uuid, b.content, p.name,
                    snippet(blocks_fts, 0, char(57344), char(57345), '…', 16),
                    bm25(blocks_fts)
             FROM blocks_fts
             JOIN blocks b ON b.id = blocks_fts.rowid
             JOIN pages p ON p.id = b.page_id
             WHERE blocks_fts MATCH ?1
             ORDER BY bm25(blocks_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts, i64::try_from(limit).unwrap_or(50)], |r| {
            Ok(SearchHit {
                block_id: r.get(0)?,
                uuid: r.get(1)?,
                content: r.get(2)?,
                page: r.get(3)?,
                snippet: r.get(4)?,
                score: -r.get::<_, f64>(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Pages whose name contains `query` (case-insensitive), journals last.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn search_pages(&self, query: &str, limit: usize) -> Result<Vec<PageRow>> {
        let pattern = format!(
            "%{}%",
            query
                .trim()
                .to_lowercase()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.conn.prepare(
            "SELECT id, name, file, journal_day FROM pages
             WHERE name_lc LIKE ?1 ESCAPE '\\'
             ORDER BY journal_day IS NOT NULL, length(name), name_lc LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, i64::try_from(limit).unwrap_or(50)], |r| {
            Ok(PageRow {
                id: r.get(0)?,
                name: r.get(1)?,
                file: r.get(2)?,
                journal_day: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn breadcrumb(&self, mut parent: Option<i64>) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut stmt = self
            .conn
            .prepare_cached("SELECT parent_id, content FROM blocks WHERE id = ?1")?;
        while let Some(id) = parent {
            let (next, content): (Option<i64>, String) =
                stmt.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            out.push(first_line(&content));
            parent = next;
        }
        out.reverse();
        Ok(out)
    }

    fn backlink_rows(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Backlink>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows: Vec<(Backlink, Option<i64>)> = stmt
            .query_map(args, |r| {
                Ok((
                    Backlink {
                        block_id: r.get(0)?,
                        uuid: r.get(1)?,
                        content: r.get(2)?,
                        page: r.get(3)?,
                        journal_day: r.get(4)?,
                        breadcrumb: Vec::new(),
                    },
                    r.get(5)?,
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        rows.into_iter()
            .map(|(mut b, parent)| {
                b.breadcrumb = self.breadcrumb(parent)?;
                Ok(b)
            })
            .collect()
    }

    /// Linked references: blocks on other pages that link, tag, embed or
    /// name the page (or one of its aliases) in a property. Journals first,
    /// newest first; then pages by name.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn backlinks(&self, name: &str) -> Result<Vec<Backlink>> {
        let (names, self_id) = self.target_names(name)?;
        let names_json = json(&names);
        self.backlink_rows(
            "SELECT DISTINCT b.id, b.uuid, b.content, p.name, p.journal_day, b.parent_id,
                    p.name_lc, b.ord
             FROM refs r
             JOIN blocks b ON b.id = r.src_block
             JOIN pages p ON p.id = b.page_id
             WHERE r.dst_page_lc IN (SELECT value FROM json_each(?1))
               AND b.page_id IS NOT ?2
             ORDER BY p.journal_day IS NULL, p.journal_day DESC, p.name_lc, b.ord",
            &[&names_json, &self_id],
        )
    }

    /// Unlinked references: blocks that mention the page name (or an
    /// alias) as plain text without linking to it.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn unlinked_references(&self, name: &str, limit: usize) -> Result<Vec<Backlink>> {
        let (names, self_id) = self.target_names(name)?;
        let names_json = json(&names);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for target in &names {
            let phrase = format!("\"{}\"", target.replace('"', ""));
            let candidates = self.backlink_rows(
                "SELECT b.id, b.uuid, b.content, p.name, p.journal_day, b.parent_id
                 FROM blocks_fts
                 JOIN blocks b ON b.id = blocks_fts.rowid
                 JOIN pages p ON p.id = b.page_id
                 WHERE blocks_fts MATCH ?1
                   AND b.page_id IS NOT ?2
                   AND NOT EXISTS (SELECT 1 FROM refs r WHERE r.src_block = b.id
                       AND r.dst_page_lc IN (SELECT value FROM json_each(?3)))
                 ORDER BY p.journal_day IS NULL, p.journal_day DESC, p.name_lc, b.ord",
                &[&phrase, &self_id, &names_json],
            )?;
            for c in candidates {
                if c.content.to_lowercase().contains(target.as_str()) && seen.insert(c.block_id) {
                    out.push(c);
                }
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    /// The page's block tree as indexed (the pre-block is left out).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn page_tree(&self, name: &str) -> Result<Option<Vec<BlockNode>>> {
        let Some(page) = self.page(name)? else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, content FROM blocks WHERE page_id = ?1 AND ord >= 0 ORDER BY ord",
        )?;
        let rows: Vec<(i64, Option<i64>, String)> = stmt
            .query_map([page.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        // Children always come after their parent in `ord`, so build bottom-up.
        let mut nodes: HashMap<i64, BlockNode> = HashMap::new();
        let mut children: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
        for (id, parent, content) in &rows {
            nodes.insert(*id, BlockNode::from_content(content));
            children.entry(*parent).or_default().push(*id);
        }
        let roots = children.get(&None).cloned().unwrap_or_default();
        Ok(Some(
            roots
                .into_iter()
                .map(|id| build(id, &mut nodes, &children))
                .collect(),
        ))
    }

    /// Journal pages, newest first.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn journals(&self, limit: usize) -> Result<Vec<JournalEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, file, journal_day FROM pages WHERE journal_day IS NOT NULL
             ORDER BY journal_day DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([i64::try_from(limit).unwrap_or(30)], |r| {
            Ok(JournalEntry {
                name: r.get(0)?,
                file: r.get(1)?,
                day: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The block with this `id::` UUID: `(page name, content)`.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn block_by_uuid(&self, uuid: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT p.name, b.content FROM blocks b JOIN pages p ON p.id = b.page_id
                 WHERE b.uuid = ?1 LIMIT 1",
                [uuid.to_ascii_lowercase()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}

fn build(
    id: i64,
    nodes: &mut HashMap<i64, BlockNode>,
    children: &HashMap<Option<i64>, Vec<i64>>,
) -> BlockNode {
    let mut node = nodes.remove(&id).unwrap_or_default();
    if let Some(kids) = children.get(&Some(id)) {
        node.children = kids.iter().map(|k| build(*k, nodes, children)).collect();
    }
    node
}

#[derive(Debug, PartialEq, Eq)]
enum FileResult {
    Indexed,
    Conflict,
    Removed,
}

fn remove_file_tx(tx: &rusqlite::Transaction<'_>, rel: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM blocks_fts WHERE rowid IN
           (SELECT b.id FROM blocks b JOIN pages p ON p.id = b.page_id WHERE p.file = ?1)",
        [rel],
    )?;
    tx.execute("DELETE FROM pages WHERE file = ?1", [rel])?;
    tx.execute("DELETE FROM files WHERE path = ?1", [rel])?;
    Ok(())
}

struct Inserter<'a, 'b> {
    tx: &'a rusqlite::Transaction<'b>,
    page_id: i64,
    ord: i64,
}

impl Inserter<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    fn insert(
        &mut self,
        content: &str,
        props: &[Property],
        refs: &[Ref],
        text: &str,
        parent: Option<i64>,
        depth: i64,
        ord: i64,
    ) -> Result<i64> {
        let info = inline::analyze(content);
        self.tx.execute(
            "INSERT INTO blocks(page_id, parent_id, ord, depth, uuid, content, marker, priority,
                                scheduled, deadline, properties)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                self.page_id,
                parent,
                ord,
                depth,
                info.uuid(),
                content,
                info.marker,
                info.priority,
                info.scheduled,
                info.deadline,
                json(&props),
            ],
        )?;
        let id = self.tx.last_insert_rowid();
        self.tx.execute(
            "INSERT INTO blocks_fts(rowid, text) VALUES (?1, ?2)",
            params![id, text],
        )?;
        let mut seen = HashSet::new();
        for r in refs {
            let (page, uuid, kind) = match r {
                Ref::Page { name, kind } => (Some(name.trim().to_lowercase()), None, *kind),
                Ref::Block { uuid, kind } => (None, Some(uuid.clone()), *kind),
            };
            if !seen.insert((page.clone(), uuid.clone())) {
                continue;
            }
            self.tx.execute(
                "INSERT INTO refs(src_block, src_page, dst_page_lc, dst_uuid, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, self.page_id, page, uuid, json(&kind).trim_matches('"')],
            )?;
        }
        Ok(id)
    }

    fn insert_tree(&mut self, blocks: &[Block], parent: Option<i64>, depth: i64) -> Result<()> {
        for block in blocks {
            let info = block.info();
            let ord = self.ord;
            self.ord += 1;
            let id = self.insert(
                &block.content,
                &info.properties,
                &info.refs,
                &info.text,
                parent,
                depth,
                ord,
            )?;
            self.insert_tree(&block.children, Some(id), depth + 1)?;
        }
        Ok(())
    }
}

fn index_file_tx(tx: &rusqlite::Transaction<'_>, graph: &Graph, rel: &str) -> Result<FileResult> {
    remove_file_tx(tx, rel)?;
    let Some(kind) = graph.classify(rel) else {
        return Ok(FileResult::Removed);
    };
    let path = graph.resolve(rel)?;
    let (text, meta) = match (std::fs::read_to_string(&path), std::fs::metadata(&path)) {
        (Ok(t), Ok(m)) => (t, m),
        (Err(e), _) | (_, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileResult::Removed);
        }
        (Err(e), _) | (_, Err(e)) => return Err(e.into()),
    };
    let doc = Document::parse(&text);
    let name = graph.page_name(rel, &doc);
    let name_lc = name.to_lowercase();
    let taken: Option<String> = tx
        .query_row(
            "SELECT file FROM pages WHERE name_lc = ?1",
            [&name_lc],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_some() {
        return Ok(FileResult::Conflict);
    }
    let page_props = doc.page_properties();
    let journal_day = match kind {
        FileKind::Journal(d) => Some(d.as_int()),
        FileKind::Page => None,
    };
    tx.execute(
        "INSERT INTO pages(name, name_lc, file, journal_day, properties) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, name_lc, rel, journal_day, json(&page_props)],
    )?;
    let page_id = tx.last_insert_rowid();
    for p in page_props
        .iter()
        .filter(|p| p.key == "alias" || p.key == "aliases")
    {
        for alias in inline::split_list(&p.value) {
            tx.execute(
                "INSERT INTO aliases(alias_lc, page_id) VALUES (?1, ?2)",
                params![alias.to_lowercase(), page_id],
            )?;
        }
    }
    let mut ins = Inserter {
        tx,
        page_id,
        ord: 0,
    };
    // The preamble (frontmatter / page properties) is indexed as a
    // pre-block with ord -1 so its text is searchable and its tags count.
    if !doc.preamble.trim().is_empty() {
        let mut refs = Vec::new();
        for p in &page_props {
            let line = format!("{}:: {}", p.key, p.value);
            refs.extend(inline::analyze(&line).refs);
        }
        let body = &doc.preamble[crate::frontmatter::body_offset(&doc.preamble)..];
        for line in body.lines() {
            if inline::property_line(line).is_none() {
                inline::scan_refs(line, &mut refs);
            }
        }
        ins.insert(
            &doc.preamble,
            &page_props,
            &refs,
            doc.preamble.trim(),
            None,
            0,
            -1,
        )?;
    }
    ins.insert_tree(&doc.blocks, None, 0)?;
    tx.execute(
        "INSERT OR REPLACE INTO files(path, mtime_ns, size) VALUES (?1, ?2, ?3)",
        params![
            rel,
            mtime_ns(&meta),
            i64::try_from(meta.len()).unwrap_or(i64::MAX)
        ],
    )?;
    Ok(FileResult::Indexed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_quotes_words() {
        assert_eq!(fts_query("a \"b"), Some("\"a\" \"b\"*".into()));
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("OR NOT"), Some("\"OR\" \"NOT\"*".into()));
    }
}
