---
title: Vault Documentation Service
tags: [architecture, vault]
status: draft
---

# Vault Documentation Service

Markdown files are the source of truth. The app writes; it never requires its own DB to read the vault.

## Layout written into the user's vault
```
<vault>/
  Next Agent/<workspace-name>/
    Tasks/2026/11/2026-11-10-add-login-form.md
    Decisions/   (agent-proposed ADR drafts, M4)
  Journal/2026-11-10.md   (appended: "- [[2026-11-10-add-login-form]] — done (auto)")
```

## Record format
See [[Task Record Template]]. Frontmatter: `type: task`, `id`, `workspace`, `mode`, `status`, `branch`, `base`, `merged`, `started`, `finished`, `model`, `tags`.

## Generation
1. On `task_completed` (and again on merge/discard to update `merged`), collect events.
2. Build deterministic sections (files changed, commands run, approvals, audit refs).
3. Ask the LLM for a short Summary + Follow-ups; fall back to deterministic text if router down.
4. Atomic write (temp + rename); never overwrite user edits below the `<!-- next-agent:end -->` marker.

## Indexing (M4)
`packages/vault`: remark-based parser for frontmatter, wikilinks, tags, blocks; SQLite FTS5 index (rebuildable cache).
