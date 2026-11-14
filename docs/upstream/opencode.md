---
title: opencode
tags: [upstream]
status: researched
---

# opencode — what we port

- Repo: https://github.com/anomalyco/opencode (formerly `sst/opencode`).
- Licence: **MIT** © 2025 opencode.
- Studied the default branch `dev`. The V2 core lives in `packages/core/src`, and the branch is mid-rewrite, so **pin a commit before porting**.
- Stack: TypeScript, Bun, Effect, SQLite.
- Integration: we port it into `crates/core` behind `AgentExecutor`. Every tool still goes through our broker and permission engine.

## 1. Context compaction (`session/compaction.ts`, ~500 lines). Top priority: we have none.

**When it triggers**
- `ceiling = window − max(15%·window, 16k)`. The 16k term applies only when the window is at least 32k.
- Compact when the estimated prompt reaches the ceiling.
- Estimate = the last response's measured usage (input + cache + output + reasoning) + the size of new text.

**When the provider rejects a request as too long**
- Retry with targets of 85%, 70%, 50% and 35% of the rejected size, then give up.

**What the summariser sees**
- Stored history is never edited. Instead a transcript for the summariser is built and shrunk, oldest exchange first, one level at a time:
  1. whole exchange
  2. tool outputs clipped to 2,000 chars from each end
  3. tool outputs removed, inputs clipped
  4. user text only
  5. "[N older exchanges omitted]"
- The newest exchange is never omitted.
- Images count as 1,500 tokens and PDFs as 2,000.

**What stays verbatim**
- The newest 15k tokens, starting at a user message so tool calls and results stay paired.
- The first kept message ID is stored as `keepID`.

**Summary format**
- Headings: Objective, Requirements, Decisions, Work State (Completed / Active / Blocked), Next Move, Relevant Files (at most 15, each with a reason), Important Context.
- Style: terse bullets, exact identifiers.
- If a summary already exists, the request is phrased as an update: newer information wins.
- If the headings are missing, nudge the model once, then fail.

**How it is used**
- Later requests start with `<conversation-checkpoint><summary>…</summary><recent-context>…</recent-context></conversation-checkpoint>`.
- Skip the optional "precompute" feature.

## 2. Edit matching (`tool/plugin/edit.ts` ~400, `file-mutation.ts`)

**Three tiers; the first tier that finds any match wins**
1. Exact match on the LF-normalised text.
2. Unicode-normalised: smart quotes, dashes and non-breaking spaces become ASCII.
3. Line-based, ignoring trailing whitespace.

**Line endings**
- Match positions map back to the original CRLF text.
- An LCS diff of old vs new keeps original endings on unchanged lines; new lines use the file's dominant ending.

**Multiple matches**
- Without `replaceAll`, more than one match is an error.
- With `replaceAll`, edits apply from last to first.

**Miss hints**
- If `newString` (at least 20 chars) is already in the file, say "may already be applied".
- Otherwise show up to 12 of the closest numbered lines (200 chars each), chosen by whitespace-insensitive voting.

**Writing**
- File lock, BOM preserved.

## 3. Shadow-git snapshots / undo (`snapshot.ts`, `git.ts`, ~300)
- A separate git repo lives outside the project at `<data>/snapshot/<projectId>/<hash(worktree)>`.
- `capture` writes a tree ID. It respects the project's ignore files and skips untracked files over 2 MiB.
- `diff` and `files` compare two tree IDs.
- `restore` refuses any path that resolves outside the worktree.
- `pack` runs at most once every 10 minutes.
- This gives per-step undo without touching the user's repo. Implement it in our git seam (`crates/core/src/worktree.rs`, the only place allowed to spawn git).

## 4. Agents and subagents (`agent.ts`, `subagent.ts`, `session/subagent-job.ts`)
- Agents are Markdown files: `.opencode/agents/<name>.md`. For us that becomes `.next-agent/agents/<name>.md`.
- Frontmatter: `mode` (primary / subagent / all), `description`, `model`, `permissions`, `steps`. The file body is the system prompt.
- Built-in agents:
  - `build`: default
  - `plan`: read-only
  - `general`: subagent; cannot spawn subagents
  - `explore`: read-only subagent
- `steps` caps the turns. The last turn removes all tools and forces a summary.
- The subagent tool runs a child session and reports back when it finishes.

## 5. Permission refinements (`permission.ts`, ~200)
- Last matching rule wins.
- When a call touches several resources, any deny wins, then any ask.
- Shell commands are split into subcommands and each part is checked. A command that can't be analysed is an error, not allowed.
- An `external_directory` action covers paths outside the project.
- "Always" never overrides a deny. Reject clears all pending approvals for the session.
- Compare each of these with our engine before adopting it. Ours already does Deny > catastrophic > mode > Allow.

## Skip
- LSP: off by default upstream, and its diagnostics hook is unfinished.
- MCP: port later using the `rmcp` crate.
- The models.dev catalog: take only its schema idea (per-model context and cache metadata).
- Prompts: tiny. Write our own.
