---
title: Hermes Agent
tags: [upstream]
status: researched
---

# Hermes Agent — what we port

- Repo: https://github.com/NousResearch/hermes-agent
- Licence: **MIT** © 2025 Nous Research
- Language: Python 3.11+. We ship no Python runtime: everything below is re-implemented in Rust.

## Rules we keep: prompt-cache invariants (`agent/AGENTS.md`)
- The system prompt is byte-stable for the whole conversation.
- New content is injected only into user messages or tool results.
- No two messages in a row have the same role.
- Compaction is the only thing allowed to change the context.

## 1. Skills (`tools/skills_tool.py`, `tools/skill_manager_tool.py`, `skills/AGENTS.md`)

**Format**
- One folder per skill: `<root>/[category/]<name>/SKILL.md`.
- Optional subfolders: `references/`, `templates/`, `scripts/`, `assets/`.
- Follows the agentskills.io standard.
- Frontmatter fields: `name`, `description`, `version`, `author`, `license`, `platforms`, `metadata.{tags,category,related_skills,config}`.

**Discovery**
- Reads only the first 4,000 characters of each SKILL.md.
- If `description` is missing, the first line of the body is used.
- Results are cached for 30 s, keyed by the folders' modification times.
- Skills can be filtered by platform and by a disabled list.

**Progressive disclosure**
- `skills_list` returns only name, description and category.
- `skill_view` returns the full body plus the skill's linked files. `file_path` fetches one file.

**Authoring (`skill_manage`)**
- Takes a batch of operations: `create`, `patch` (old/new string), `write_file`, `remove_file`, `delete` (with optional `absorbed_into`).
- The batch is atomic: if anything fails, everything rolls back.
- Name rule: lowercase letters, digits and `.-_`, starting with a letter or digit, at most 64 characters.
- Size limits: SKILL.md at most 100,000 characters; each other file at most 1 MiB.
- Skills the agent creates are marked `created_by: agent`.

**Usage tracking and the curator**
- Each skill records `use/view/patch` counts, `last_activity_at`, a `state` (`active` / `stale` / `archived`) and a `pinned` flag.
- The curator only touches agent-created skills that are not pinned.
- It archives stale skills into `.archive/` and never deletes them.

**For us**
- Skills live in the vault. Usage counts go in a SQLite table.

## 2. Curated memory (`tools/memory_tool.py`)

**Files**
- `MEMORY.md`: the agent's notes, about 2,200 characters at most.
- `USER.md`: the user profile, about 1,375 characters at most.

**Editing**
- The model can `add`, `patch` and `remove` entries.
- The batch is atomic. The character limit is checked against the final result.

**How it reaches the model**
- Both files are injected as a **frozen snapshot when the session starts**.
- Writes during a session reach the disk but do not change the current prompt.
- For us, writes are tool calls through the broker, so the permission engine gates them.

## 3. Session store and search (`hermes_state*.py`, `tools/session_search_tool.py`)

**Storage**
- SQLite in WAL mode.
- Tables: `sessions`, `messages` (role, content, tool_calls, tool_call_id, token_count, active flag, `message_uid`), and `system_prompts(hash, prompt)`, which stores each system prompt once by content hash.
- `messages_fts`: an external-content FTS5 index over a view that truncates tool output, kept in sync by triggers. A trigram index is optional, for CJK text.

**`session_search` tool modes**
- **discovery**: an FTS query returning 1–10 results (default 3). Scans at most 300 rows and removes duplicates by the session's lineage root. Cron sessions rank lower.
- **scroll**: a window of messages around one message.
- **read**: a whole session, or only its first and last messages if it is long.
- **browse**: the most recent sessions.

## 4. Background review (`agent/background_review.py`)
- After a turn finishes, fork a read-only agent onto a snapshot of the conversation.
- The fork gets a memory prompt, a skills prompt, or both. Answering "nothing to save" is valid.
- Its tools are limited to memory, skills, `read_file` and `search`. It is capped at 16 iterations and a token budget.
- When deciding where to save a skill, it prefers, in order:
  1. patching a skill already loaded
  2. patching an umbrella skill
  3. adding a support file
  4. creating a new skill
- The prompts are MIT-licensed, so we can reuse them with credit.

## 5. Context compaction
- Hermes compacts at 50% of the context window: it prunes tool results first, then has the model write a summary.
- We use opencode's algorithm instead (see [[opencode]]).

## 6. Subagents (`tools/delegate_tool*.py`)
- Each task has `goal`, `context`, `output_schema` and `group`.
- Actions: spawn, list, steer, stop, continue.
- A child inherits the parent's tools, minus clarify, memory and cron. It also loses delegate unless it is an orchestrator.
- Limits: depth, concurrency and iterations.
- The parent sees only `{status, exit_reason, truncated, schema_valid}`.
- **Merge this with opencode's subagent tool. Build one subagent tool, not two.**

## 7. Cron (`cron/jobs.py`, `cron/scheduler.py`)

**Schedule formats**
- A duration (`30m`).
- `every 2h` / `every monday 9am`.
- 5-field cron.
- A one-shot ISO date.

**Job fields**
- `skills`, `model`, `prompt`, `context_from`, `workdir`, `timezone`, `grace_minutes`, and delivery settings.

**Running jobs**
- Each tick takes a lock and **advances `next_run_at` before dispatching**, so a job runs at most once even if the app crashes.
- Missed runs are caught up within a window of half the period, clamped to between 120 s and 2 h.
- Each cron run gets its own session and skips memory.
- Jobs fire only while the app is running.

## 8. Approval ideas to cherry-pick
- A blocklist of hard-banned commands.
- An optional "guardian" model that approves, denies or escalates.
- A circuit breaker after repeated denials.

## Skip
- The 25+ messaging gateways ([[OpenClaw]] covers channels).
- Terminal backends, browser and computer use, voice, the TUI and Electron apps, RL tooling, and the Python code-execution tool.
