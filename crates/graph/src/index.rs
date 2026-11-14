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

use crate::inline::{self, BlockInfo, Ref};
use crate::layout::{FileKind, Graph, GraphError};
use crate::model::BlockNode;
use crate::parse::{Block, Document};

/// Bump when the schema changes; older caches are dropped and rebuilt.
const SCHEMA_VERSION: &str = "3";

/// Snippet highlight markers (Unicode private use), so the UI can split the
/// snippet safely instead of rendering HTML.
pub const MATCH_START: char = '\u{E000}';
pub const MATCH_END: char = '\u{E001}';

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE files(path TEXT PRIMARY KEY, mtime_ns INTEGER NOT NULL, size INTEGER NOT NULL);
-- Files skipped because their page name was taken; retried after changes.
CREATE TABLE pending(path TEXT PRIMARY KEY, name_lc TEXT NOT NULL);
CREATE INDEX pending_name ON pending(name_lc);
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

/// `(mtime, size)`: what `sync` compares to decide a file changed.
fn stamp(meta: &std::fs::Metadata) -> (i64, i64) {
    (
        mtime_ns(meta),
        i64::try_from(meta.len()).unwrap_or(i64::MAX),
    )
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
                 DROP TABLE IF EXISTS meta; DROP TABLE IF EXISTS pending;",
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
        self.clear()?;
        self.sync(graph)
    }

    fn clear(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "BEGIN;
             DELETE FROM blocks_fts; DELETE FROM refs; DELETE FROM aliases;
             DELETE FROM blocks; DELETE FROM pages; DELETE FROM files;
             DELETE FROM pending; DELETE FROM meta WHERE key = 'config';
             COMMIT;",
        )?;
        Ok(())
    }

    /// Incremental update: re-index files whose size or mtime changed and
    /// drop files that disappeared.
    ///
    /// # Errors
    /// SQLite or I/O failures.
    pub fn sync(&mut self, graph: &Graph) -> Result<IndexStats> {
        // Page names depend on the config (journal formats, file-name
        // format): an index built under another config starts over.
        let fingerprint = format!("{:?}", graph.config());
        let built_with: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'config'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if built_with.as_deref() != Some(fingerprint.as_str()) {
            self.clear()?;
            self.conn.execute(
                "INSERT INTO meta(key, value) VALUES ('config', ?1)",
                [&fingerprint],
            )?;
        }
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
            // `files()` already skipped symlinks and ignored paths.
            let meta = std::fs::metadata(graph.root().join(rel))?;
            if known.get(rel) == Some(&stamp(&meta)) {
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
        let freed = page_name_of(&tx, rel)?;
        let result = index_file_tx(&tx, graph, rel)?;
        retry_pending(&tx, graph, freed.as_deref(), rel)?;
        tx.commit()?;
        Ok(result == FileResult::Indexed)
    }

    /// Forget one file.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn remove_file(&mut self, graph: &Graph, rel: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let freed = page_name_of(&tx, rel)?;
        remove_file_tx(&tx, rel)?;
        retry_pending(&tx, graph, freed.as_deref(), rel)?;
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

    /// Ancestor first lines, outermost first. `seen` memoises
    /// `id → (parent, first line)` across the rows of one query.
    fn breadcrumb(
        &self,
        mut parent: Option<i64>,
        seen: &mut HashMap<i64, (Option<i64>, String)>,
    ) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut stmt = self
            .conn
            .prepare_cached("SELECT parent_id, content FROM blocks WHERE id = ?1")?;
        while let Some(id) = parent {
            let (next, line) = if let Some(hit) = seen.get(&id) {
                hit.clone()
            } else {
                let (next, content): (Option<i64>, String) =
                    stmt.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
                let hit = (next, first_line(&content));
                seen.insert(id, hit.clone());
                hit
            };
            out.push(line);
            parent = next;
        }
        out.reverse();
        Ok(out)
    }

    /// Run a backlink query, keep the rows `keep` accepts (at most
    /// `limit`), then add breadcrumbs to the survivors only.
    fn backlink_rows(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
        limit: usize,
        mut keep: impl FnMut(&Backlink) -> bool,
    ) -> Result<Vec<Backlink>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows: Vec<(Backlink, Option<i64>)> = Vec::new();
        let mut cursor = stmt.query(args)?;
        while let Some(r) = cursor.next()? {
            let b = Backlink {
                block_id: r.get(0)?,
                uuid: r.get(1)?,
                content: r.get(2)?,
                page: r.get(3)?,
                journal_day: r.get(4)?,
                breadcrumb: Vec::new(),
            };
            if keep(&b) {
                rows.push((b, r.get(5)?));
                if rows.len() >= limit {
                    break;
                }
            }
        }
        let mut seen = HashMap::new();
        rows.into_iter()
            .map(|(mut b, parent)| {
                b.breadcrumb = self.breadcrumb(parent, &mut seen)?;
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
            usize::MAX,
            |_| true,
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
        let mut out: Vec<Backlink> = Vec::new();
        let mut seen = HashSet::new();
        for target in &names {
            if out.len() >= limit {
                break;
            }
            let phrase = format!("\"{}\"", target.replace('"', ""));
            let found = self.backlink_rows(
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
                limit - out.len(),
                |c| c.content.to_lowercase().contains(target.as_str()) && seen.insert(c.block_id),
            )?;
            out.extend(found);
        }
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

/// Lowercased page name a file currently holds in the index.
fn page_name_of(tx: &rusqlite::Transaction<'_>, rel: &str) -> Result<Option<String>> {
    Ok(tx
        .query_row("SELECT name_lc FROM pages WHERE file = ?1", [rel], |r| {
            r.get(0)
        })
        .optional()?)
}

/// After `rel` changed or went away, give the page name it held (if now
/// free) to the first file that was skipped for it.
fn retry_pending(
    tx: &rusqlite::Transaction<'_>,
    graph: &Graph,
    freed: Option<&str>,
    rel: &str,
) -> Result<()> {
    let Some(freed) = freed else { return Ok(()) };
    let waiting: Vec<String> = {
        let mut stmt = tx.prepare_cached(
            "SELECT path FROM pending WHERE name_lc = ?1 AND path != ?2
             AND NOT EXISTS (SELECT 1 FROM pages WHERE name_lc = ?1) ORDER BY path",
        )?;
        let rows = stmt.query_map([freed, rel], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for other in waiting {
        if index_file_tx(tx, graph, &other)? == FileResult::Indexed {
            break; // the name is taken again
        }
    }
    Ok(())
}

fn remove_file_tx(tx: &rusqlite::Transaction<'_>, rel: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM blocks_fts WHERE rowid IN
           (SELECT b.id FROM blocks b JOIN pages p ON p.id = b.page_id WHERE p.file = ?1)",
        [rel],
    )?;
    tx.execute("DELETE FROM pages WHERE file = ?1", [rel])?;
    tx.execute("DELETE FROM files WHERE path = ?1", [rel])?;
    tx.execute("DELETE FROM pending WHERE path = ?1", [rel])?;
    Ok(())
}

struct Inserter<'a, 'b> {
    tx: &'a rusqlite::Transaction<'b>,
    page_id: i64,
    ord: i64,
}

impl Inserter<'_, '_> {
    fn insert(
        &mut self,
        content: &str,
        info: &BlockInfo,
        parent: Option<i64>,
        depth: i64,
        ord: i64,
    ) -> Result<i64> {
        self.tx
            .prepare_cached(
                "INSERT INTO blocks(page_id, parent_id, ord, depth, uuid, content, marker,
                                    priority, scheduled, deadline, properties)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?
            .execute(params![
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
                json(&info.properties),
            ])?;
        let id = self.tx.last_insert_rowid();
        self.tx
            .prepare_cached("INSERT INTO blocks_fts(rowid, text) VALUES (?1, ?2)")?
            .execute(params![id, info.text])?;
        let refs = &info.refs;
        let mut seen = HashSet::new();
        for r in refs {
            let (page, uuid, kind) = match r {
                Ref::Page { name, kind } => (Some(name.trim().to_lowercase()), None, *kind),
                Ref::Block { uuid, kind } => (None, Some(uuid.clone()), *kind),
            };
            if !seen.insert((page.clone(), uuid.clone())) {
                continue;
            }
            self.tx
                .prepare_cached(
                    "INSERT INTO refs(src_block, src_page, dst_page_lc, dst_uuid, kind)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?
                .execute(params![
                    id,
                    self.page_id,
                    page,
                    uuid,
                    json(&kind).trim_matches('"')
                ])?;
        }
        Ok(id)
    }

    fn insert_tree(&mut self, blocks: &[Block], parent: Option<i64>, depth: i64) -> Result<()> {
        for block in blocks {
            let ord = self.ord;
            self.ord += 1;
            let id = self.insert(&block.content, &block.info(), parent, depth, ord)?;
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
    let page_props = doc.page_properties();
    let name = graph.page_name_from(rel, &page_props);
    let name_lc = name.to_lowercase();
    let taken: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pages WHERE name_lc = ?1)",
        [&name_lc],
        |r| r.get(0),
    )?;
    if taken {
        tx.execute(
            "INSERT OR REPLACE INTO pending(path, name_lc) VALUES (?1, ?2)",
            [rel, name_lc.as_str()],
        )?;
        return Ok(FileResult::Conflict);
    }
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
        let body = &doc.preamble[crate::frontmatter::body_offset(&doc.preamble)..];
        // Stored as readable `key:: value` lines plus any text, not raw YAML;
        // analysing that text yields the property refs (tags, alias, links).
        let text_lines: Vec<&str> = body
            .lines()
            .filter(|l| !l.trim().is_empty() && inline::property_line(l).is_none())
            .collect();
        let shown: Vec<String> = page_props
            .iter()
            .map(|p| format!("{}:: {}", p.raw_key, p.value))
            .chain(text_lines.iter().map(|l| (*l).to_string()))
            .collect();
        let content = shown.join("\n");
        let normalised: Vec<String> = page_props
            .iter()
            .map(|p| format!("{}:: {}", p.key, p.value))
            .chain(text_lines.iter().map(|l| (*l).to_string()))
            .collect();
        let mut info = inline::analyze(&normalised.join("\n"));
        info.properties.clone_from(&page_props);
        info.text.clone_from(&content);
        ins.insert(&content, &info, None, 0, -1)?;
    }
    ins.insert_tree(&doc.blocks, None, 0)?;
    tx.execute(
        "INSERT OR REPLACE INTO files(path, mtime_ns, size) VALUES (?1, ?2, ?3)",
        params![rel, stamp(&meta).0, stamp(&meta).1],
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
