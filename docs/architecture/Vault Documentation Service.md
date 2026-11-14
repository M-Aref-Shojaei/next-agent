---
title: Vault Documentation Service
tags: [architecture, vault]
status: draft
---

# Vault Documentation Service

Markdown files are the source of truth. The app writes; it never requires its own DB to read the vault.

## Layout written into the user's vault
The vault is a Logseq graph (see [[Notes Engine]]):
```
<vault>/
  logseq/config.edn            (created when missing: triple-lowbar, yyyy_MM_dd journals)
  pages/Next Agent___<workspace>___Tasks___2026-11-10-add-login-form.md
        → page "Next Agent/<workspace>/Tasks/2026-11-10-add-login-form"
  journals/2026_11_10.md       (block appended: "- [[Next Agent/<workspace>/Tasks/2026-11-10-add-login-form]] — done (auto)")
  assets/
```
- Journal file names follow the vault's `:journal/file-name-format`.
- A second task with the same date and slug gets `-2`, `-3`, …
- File names are produced by `next_agent_graph::page_file_name` (Rust) and `pageFileName` (TS). Both are checked against `fixtures/vault/page-file-names.tsv`.
- Records written before this layout (`Next Agent/<ws>/Tasks/YYYY/MM/…`, `Journal/…`) are left where they are and not moved.

## Record format
See [[Task Record Template]]. Frontmatter: `type: task`, `id`, `title` (the Logseq page name), `task` (the human title), `workspace`, `mode`, `status`, `branch`, `base`, `merged`, `started`, `finished`, `model`, `tags`.

## Generation
1. On `task_completed` (and again on merge/discard to update `merged`), collect events.
2. Build deterministic sections (files changed, commands run, approvals, audit refs).
3. Ask the LLM for a short Summary + Follow-ups; fall back to deterministic text if router down.
4. Atomic write (temp + rename); never overwrite user edits below the `<!-- next-agent:end -->` marker.

## Indexing
`crates/graph` parses the vault as a Logseq graph and keeps a rebuildable SQLite FTS5 index. See [[Notes Engine]].
