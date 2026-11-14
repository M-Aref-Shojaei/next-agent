//! Notes engine: Logseq-compatible Markdown graphs.
//!
//! Format follows Logseq (AGPL-3.0, © Logseq contributors) file graphs as of
//! tag 0.10.9 (commit c67b8b5, pinned in `upstream.lock.json`). See
//! `docs/upstream/Logseq.md` and `docs/architecture/Notes Engine.md`.
//!
//! - [`Document`] / [`Block`]: lossless parser and writer for one page file.
//! - [`Graph`]: folder layout, `config.edn`, file names, journals.
//! - [`Index`]: rebuildable SQLite cache with full-text search, backlinks
//!   and journals; [`GraphWatcher`] keeps it fresh.
//! - [`PageView`] / [`BlockNode`]: the JSON shape the UI reads and saves.

pub mod frontmatter;
pub mod index;
pub mod inline;
pub mod layout;
pub mod model;
pub mod parse;
pub mod watch;

pub use index::{Backlink, Index, IndexError, IndexStats, JournalEntry, SearchHit};
pub use inline::{BlockInfo, Property, Ref, RefKind};
pub use layout::{
    format_date, page_file_name, parse_date, title_from_file_body, write_atomic, Date, FileKind,
    Graph, GraphConfig, GraphError,
};
pub use model::{BlockNode, PageView};
pub use parse::{Block, Document};
pub use watch::GraphWatcher;
