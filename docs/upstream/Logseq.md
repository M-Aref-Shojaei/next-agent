---
title: Logseq
tags: [upstream]
status: in progress
---

# Logseq — what we build in

> **Status:** steps 1–2 (parser/writer, index) are built in `crates/graph`; steps 3–7 (outliner, backlinks and journals, queries, graph view, Mermaid) are the Notes UI in `apps/desktop/src/lib/notes` and `routes/notes`, pinned to tag 0.10.9 @ c67b8b5. See [[Notes Engine]].

- Repo: https://github.com/logseq/logseq
- Licence: **AGPL-3.0**. It is compatible with our licence (see [[ADR-0009 Built-in Upstreams by Extraction, AGPL-3.0]]).
- Version: we target the **file-graph / Markdown** format. `master` is moving to a database-backed graph (beta); we don't follow it.
- Logseq is written in ClojureScript. We re-implement it:
  - **Rust** owns files, parsing, the SQLite index and writes.
  - **Svelte** owns rendering and the editor.

## On-disk format to read and write (`deps/graph-parser`: `extract.cljc`, `block.cljs`, `property.cljs`)

### Folder layout
- `pages/*.md`
- `journals/yyyy_MM_dd.md` (default name, set by `:journal/file-name-format`)
- `assets/`
- `logseq/config.edn`

### Blocks
- A block is a `- ` bullet.
- Children are indented under their parent (tab by default; continuation lines are indented two spaces past the bullet).
- A block's text continues on lines indented to its text column.

### Block properties
- Written as `key:: value` lines right under the block's first line.
- Keys are lowercased, and spaces and `_` become `-` (verified in `block.cljs`; `/` is kept).
- `id::` holds a UUID. `custom_id` and `custom-id` mean the same thing.
- `collapsed:: true` marks a folded block.
- These properties are hidden in the UI: `id`, `collapsed`, `heading`, `background-color`, `created-at`, `updated-at`, `query-*`, `hl-*`.

### Page properties
- The first block holds `key:: value` lines, or YAML frontmatter.
- Special keys: `title::`, `alias::`, `tags::`, `icon`, `template`, `public`, `exclude-from-graph-view`.
- A page's name comes from `title::` first, then the filename, then the first heading.

### Links and embeds
- `((uuid))` refers to a block. The target block must carry `id:: uuid`.
- `{{embed [[Page]]}}` and `{{embed ((uuid))}}` embed a page or block.
- Other `{{macro}}` calls are kept as-is.
- `[[Page]]`, `#tag` and `#[[multi word]]` are page references.

### Tasks
- Markers: `TODO`, `DOING`, `DONE`, `NOW`, `LATER`, `WAITING`, `CANCELED`/`CANCELLED`.
- Priority: `[#A]`.
- `SCHEDULED: <2026-11-14 Sat .+1w>` and `DEADLINE: <…>` each sit on their own line.
- `:LOGBOOK:` drawers hold time tracking.

### File names
- `:file/name-format :triple-lowbar`: `___` in a filename means `/` in the page name. Other illegal characters are percent-encoded.

### Our vault (since the notes engine)
- Task records are pages under `pages/` named `Next Agent/<workspace>/Tasks/<date>-<slug>`, stored with triple-lowbar file names. The YAML frontmatter is kept (Logseq reads it as page properties).
- Journal links are blocks in `journals/yyyy_MM_dd.md`, or whatever the vault's `:journal/file-name-format` says.

### Round-trip rule
- Anything the parser doesn't understand is kept as opaque text and written back unchanged.

## MVP features in build order
1. A **Rust parser and writer** for blocks, properties, `id::`, links, tags and task markers. Includes a round-trip test corpus.
2. A **SQLite index** with these tables, refreshed incrementally by a `notify` file watcher:
   - `pages`
   - `blocks(parent, order, page)`
   - `refs(src, dst)`
   - `properties`
   - FTS5 search with `bm25` ranking and `snippet`
3. The **outliner editor** in Svelte. Each block gets its own small editor. Keys: indent, outdent, move, collapse. Includes undo/redo.
4. A **backlinks / linked references** panel and a **journals feed**.
5. **Simple queries**: `{{query (and [[x]] (task TODO))}}` compiled to SQL. Supports `and`, `or`, `not`, `task`, `page`, `property`, `between` and `priority`.
6. A **graph view**: `d3-force` with a canvas renderer, fed by the `refs` table.
7. **Mermaid** rendering of ```` ```mermaid ```` blocks.

## Later or never
- Advanced Datalog queries.
- Whiteboards (built on a tldraw fork; tldraw has its own licence).
- PDF annotation (`pdfjs-dist`).
- The plugin API.

## Reference
- `logseq/mldoc` (AGPL, OCaml compiled to JS) is the exact upstream parser. Use it as a reference or a test oracle. Our parser is Rust.
- The research estimate for one developer is about 2–3 weeks for steps 1–2, and about 8–10 weeks through step 6.
