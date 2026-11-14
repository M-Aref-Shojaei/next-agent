---
title: Notes Engine
tags: [architecture, vault, logseq]
status: draft
---

# Notes Engine

`crates/graph` (`next-agent-graph`) reads and writes **Logseq Markdown file graphs** and keeps a SQLite index of them. It is steps 1–2 of the Logseq MVP in [[Logseq]]. The format follows Logseq **0.10.9 @ c67b8b5** (the last file-graph release), pinned in `upstream.lock.json`. Licence: AGPL-3.0, see [[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]].

## Modules
| Module | Job |
|---|---|
| `parse.rs` | `Document` / `Block`: lossless parser and writer for one page file |
| `inline.rs` | Facts derived from block text: properties, markers, priority, `SCHEDULED`/`DEADLINE`, refs, tags, embeds |
| `frontmatter.rs` | YAML frontmatter as page properties |
| `layout.rs` | `Graph`: folders, `logseq/config.edn`, triple-lowbar file names, journal dates, atomic writes |
| `index.rs` | `Index`: SQLite cache with FTS5. The only file in the crate that uses `rusqlite` |
| `watch.rs` | `GraphWatcher`: `notify` watcher with debounce that re-indexes changed files |
| `model.rs` | `PageView` / `BlockNode`: the JSON the UI reads and saves |

## Round-trip rule
- Every parsed block keeps the exact bytes it came from. `render()` copies them back when the block's content and depth are unchanged, so **parse → render is byte-for-byte**. This holds for any input, including CRLF, a missing final newline, trailing spaces and unknown syntax.
- A changed or new block is rendered in Logseq's style: tab nesting, and continuation lines indented two spaces past the bullet.
- `replace_blocks(tree)` takes the tree back from the editor. It matches blocks by `(depth, content)` along document order (LCS), so blocks the user didn't touch keep their bytes.
- Text before the first bullet (frontmatter, page properties, plain Markdown) is the **preamble** and is kept verbatim.
- Tests: 49 corpus files in `fixtures/logseq-graph` (a complete graph) and a proptest suite (`crates/graph/tests/roundtrip_props.rs`): random text round-trips, random trees render and re-parse, and edits stay local.

## Format coverage
- Blocks: `- ` bullets (`-` alone is an empty block). Nesting with tabs or any number of spaces. Multi-line bodies. Bullets inside ```` ``` ```` / `~~~` fences are text.
- Block properties: `key:: value`. Keys are lowercased and spaces/`_` become `-`. `id::` (also `custom_id::`/`custom-id::`) gives the UUID. Hidden built-ins (`id`, `collapsed`, `heading`, …) are left out of search text.
- Page properties: YAML frontmatter, `key:: value` lines before the first bullet, or a first block made only of properties. A page's name comes from `title::` first, then the journal date, then the file name.
- Refs: `[[page]]` (nested too), `#tag`, `#[[multi word]]`, `((uuid))`, `{{embed [[page]]}}`, `{{embed ((uuid))}}`. Values of `tags::` and `alias::` are page refs. Inline code and `{{query …}}` arguments are not refs. Other macros are kept verbatim.
- Tasks: `TODO DOING DONE NOW LATER WAITING WAIT CANCELED CANCELLED STARTED IN-PROGRESS`, `[#A]`–`[#C]`, `SCHEDULED: <…>`, `DEADLINE: <…>`. `:LOGBOOK:` and other drawers are kept verbatim and left out of search.
- Layout: `pages/` (subfolders too), `journals/` named by `:journal/file-name-format` (default `yyyy_MM_dd`), journal titles by `:journal/page-title-format` (default `MMM do, yyyy`), `:hidden`, and ignored paths (`logseq/bak`, dotfiles). `:file/name-format :triple-lowbar` file names are ported from `tri-lb-file-name-sanity`; legacy names are read only. `Graph::init` creates the folders and a minimal `logseq/config.edn`.

## Index (SQLite, rebuildable)
- Tables: `pages(name, name_lc, file, journal_day, properties)`, `blocks(page_id, parent_id, ord, depth, uuid, content, marker, priority, scheduled, deadline, properties)`, `refs(src_block → dst_page_lc | dst_uuid, kind)`, `aliases`, `files(mtime, size)`, and `blocks_fts` (FTS5, `unicode61 remove_diacritics 2`).
- The preamble is indexed as a pre-block with `ord = -1`, so frontmatter tags count as references.
- Queries: `search` (bm25 order, `snippet` with U+E000/U+E001 match markers, every word must match, the last as a prefix), `search_pages`, `backlinks` (through aliases, case-insensitive, journals newest first), `unlinked_references` (text mentions without a link), `page_tree`, `journals`, `block_by_uuid`.
- `sync` re-indexes files whose mtime or size changed. `rebuild` starts from scratch. The watcher debounces events (300 ms in the app) and calls `index_file`, or `sync` when a folder moves or `config.edn` changes.
- The database lives in the app data folder (`graph-index/graph-<hash>.sqlite`), never in the vault. The Markdown stays the source of truth.

## Desktop commands (for the Wave 2 notes UI)
All in `apps/desktop/src-tauri/src/notes.rs`. Arguments and results are camelCase JSON. Types are in `packages/vault/src/graph.ts`.

| Command | Args | Returns |
|---|---|---|
| `graph_open` | `path` | `GraphInfo { root, pages, blocks, createdConfig, stats }`. Creates the layout if missing, syncs the index, starts the watcher |
| `graph_search` | `q`, `limit?` (50) | `SearchHit[] { page, blockId, uuid, content, snippet, score }` |
| `graph_page` | `name` | `PageView { name, file, journalDay, properties, preamble, blocks: BlockNode[] }`. Read from the file. `file: null` for a page that is only referenced |
| `graph_backlinks` | `name` | `{ linked: Backlink[], unlinked: Backlink[] }`, each `{ page, journalDay, blockId, uuid, content, breadcrumb }` |
| `graph_journals` | `limit?` (30) | `JournalEntry[] { name, file, day }`, newest first |
| `graph_save_page` | `name`, `tree: BlockNode[]` | the saved `PageView`. Only `content` and `children` are read. Creates the page or journal file when missing (adds `title::` when the file name can't carry the title) |
| `graph_rebuild` | — | `IndexStats` |

The app emits a `graph-changed` event (payload: changed graph-relative paths; empty after a full sync) when the watcher re-indexes files.

## Known gaps
- No NFC normalisation of page names (Logseq applies it).
- `*`/`+` bullets and org-mode files are treated as text. Org `:PROPERTIES:` drawers are kept but not read as properties.
- An unclosed code fence swallows the rest of the file into one block (mldoc behaves much the same).
- Editing a block drops trailing blank lines inside it. Blocks the user didn't touch are unaffected.
- Legacy (`:legacy`) file names are read but never written. New pages always use triple-lowbar.
- Two files with the same page name: the second is reported in `IndexStats.conflicts` and not indexed.
- Namespace parent pages (`a` for `a/b`) are not created as rows. They resolve as referenced pages.
