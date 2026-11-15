---
title: Agent Execution
tags: [architecture, agents]
status: draft
---

# Agent Execution

## AgentExecutor trait
```rust
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    fn id(&self) -> &'static str;
    async fn start(&self, task: TaskSpec, tools: ToolBrokerHandle)
        -> Result<BoxStream<'static, AgentEvent>>;
    async fn cancel(&self, task_id: TaskId) -> Result<()>;
}
```
- `TaskSpec`: prompt, workspace, mode, model, history, system prompt (includes mode description so the model knows e.g. `plan` = produce a plan).
- `ToolBrokerHandle`: the *only* way an executor performs side effects: `call(ToolRequest) -> ToolResult` (awaits approvals internally).

### As built (M1)
The trait is a **synchronous step machine** (`start(spec) -> AgentStep`, `next(result) -> AgentStep`) driven by `run_task_with` on a dedicated OS thread, so the `!Sync` SQLite store and blocking tools never stall the async runtime. The spec's async surface maps to `control::spawn_task(TaskLaunch) -> TaskHandle`:
- `start → stream<AgentEvent>`: `TaskHandle::events()` (std `Receiver`, events already persisted);
- `cancel(task_id)`: `TaskHandle::cancel()` — stops before the next step, kills a running command's process group, resolves a pending approval as denied;
- `respond(approval_id, decision)`: `TaskControl::respond(id, Approval::{Allow, Deny, AlwaysForWorkspace})`.

All events flow through one `Emitter` (sequence numbers + ordered sinks: `StoreSink` first, then the UI forwarder), shared by runner, broker, approver and tool executor. A failing persistence sink stops forwarding and fails the task.

## Implementations
| Executor | Milestone | Notes |
|---|---|---|
| `MockExecutor` | M1 | Scripted events; used by tests and UI dev |
| `BuiltinExecutor` | M1 | Rust tool-calling loop over OpenAI-compatible chat completions via [[Router Integration]] |
| `OpencodeExecutor` | M2 | Via `services/agent-gateway` (Node) speaking JSON-RPC over stdio; opencode's own tools disabled/redirected to the broker |
| `HermesExecutor` | M2 opt. | Via `services/hermes-adapter` (uv); memory/skills `state.db` kept per workspace |

## Tool loop (BuiltinExecutor)
1. Send messages + tool schemas to router (streaming).
2. Stream text deltas as `AgentEvent::MessageDelta`.
3. On tool call → `AgentEvent::ToolRequested` → broker → `PermissionDecided` → (maybe `ApprovalRequested` … `ApprovalResolved`) → `ToolStarted`/`ToolOutput`/`ToolFinished`.
4. Append result, loop until no tool calls or step budget reached (default 50) → `TaskCompleted`.

## Tool set
`read_file`, `list_dir`, `search`, `write_file`, `edit`, `apply_patch`, `run_command` (streamed stdout/stderr, timeout, kill on cancel), `http_fetch`, `subagent`; `ask_user` is refused for now.
- `edit` (old_string / new_string / replace_all) and `apply_patch` use tiered matching (`edit_match.rs`, ported from opencode): exact → Unicode-normalised (smart quotes, dashes, NBSP) → line-based ignoring trailing whitespace, on the LF view of the file with positions mapped back. CRLF / mixed endings and a UTF-8 BOM are preserved. More than one match without `replace_all` is an error; a miss returns "may already be applied" or the closest numbered lines. Patch hunks that do not apply in place are located by the same matcher, nearest to their line number. Policy treats both as `write_file`.

## Engine features (Wave 1 session 2)
Ported from opencode @ `cbe71595` and Hermes Agent @ `a9ee443d` (see `upstream.lock.json`, `NOTICE.md`).

**Agent profiles** (`profiles.rs`). Built-ins `build` (default), `plan` (read-only, default in plan mode), `general` (subagent, cannot spawn) and `explore` (read-only subagent), plus `.next-agent/agents/<name>.md` files (frontmatter `mode`, `description`, `model` or `model_role`, `permissions`, `steps`; body joins the system prompt). Permissions only tighten: denied tools are not offered, and every call carries the chain's `Floor` (`Ask`/`Deny`) to the broker, which applies it on top of the engine's decision (`SqliteBroker::handle_floored`). A workspace file reusing a built-in name merges with it, keeping the stricter rule. `steps` caps model turns; the last turn has no tools and asks for a summary.

**Context compaction** (`compaction.rs`). Estimate = last measured `usage` + chars/4 since; compact when it reaches `window − max(15%, 16k for windows ≥ 32k)` (default window 128k, `with_context_window`). The summariser sees the conversation unchanged, else the history before the 15k-token verbatim tail, else a text transcript shrunk oldest-first through five levels (newest exchange never omitted); "too long" rejections retry at 85/70/50/35%. The summary must use the opencode template (one nudge). The new conversation is the byte-identical system prompt + one `<conversation-checkpoint>` user message. A provider overflow on a normal turn compacts and retries once. Emits `context_compacted`. Summaries use `with_summary_client` (the gateway's `summary` role) or the task's client.

**Per-step undo** (`worktree.rs` `ShadowRepo`, `snapshot.rs`). With `spawn_task_with(launch, TaskOptions { snapshots: Some(Box::new(GitSnapshots::open(data_dir, sandbox_root)?)) })`, the broker captures the work tree into a private `GIT_DIR` (`<data>/snapshot/<hash>`) before each allowed mutating call and records it in `agent_step_snapshots` (event `step_snapshot`). `undo_last_step(&store, task_id)` restores the newest not-undone capture (paths outside the work tree are refused) — call it only while the task is not running.

**Subagents** (`subagent.rs`, one tool merging opencode's subagent tool and Hermes' delegate). `subagent {tasks: [{goal, context?, agent?, output_schema?}]}` (≤ 3 per call, depth ≤ 1 by default) runs child executors interleaved; their calls leave through the parent's steps (call ids `sub1.<id>`), so they hit the same runner, broker, permission engine, approval channel and sandbox root. A child's profile chain includes its ancestors (it can never do more than its parent) and loses blocklisted tools. The parent gets `{status, exit_reason, truncated, schema_valid, summary}` per child; `subagent_started` / `subagent_finished` and all child tool events appear in the parent task's timeline.

**Prompt-cache invariants** (Hermes `agent/AGENTS.md`). The system prompt is fixed per task (hash-checked before every request); new content enters only as user or tool messages; no two user or assistant messages are adjacent (`compaction::push_user` merges); only compaction rewrites history. `builtin::validate_transcript` runs before every request; a violation fails the task instead of sending a cache-busting prompt.

## Memory and learning (Wave 2 session 7)
Ported from Hermes Agent @ `fa1aab31` (`crates/core/src/learning/`, desktop `src-tauri/src/learning.rs`, Settings → Memory and skills).

**Tools.** `memory`, `skills_list`, `skill_view`, `skill_manage`, `session_search` reach the broker as `ToolRequest::Learn`. Policy judges the writers (`memory`, `skill_manage`) as `write_file` on their real target (the `MEMORY.md`/`USER.md` file, or the one skills folder the call writes) and the readers as `read_file` of the sandbox's `.next-agent`. Plan mode and the `plan`/`explore` profiles therefore never write memory or skills; manual and auto ask (memory lives outside the workspace), bypass allows. Subagents get none of them.

**Memory.** `<app data>/memory/<workspace id>/{MEMORY.md,USER.md}`, entries split by `\n§\n`, caps 2,200 / 1,375 characters checked on the batch result, atomic writes, threat-pattern scan on save. A **frozen snapshot** (plus the skills index) is put in front of the first user message when a task starts and re-attached to the compaction checkpoint; the system prompt only gains static guidance, so it stays byte-stable. The snapshot is fenced as data (`<memory-snapshot>`; entries cannot close the fence; flagged entries are blanked).

**Skills.** `<workspace>/.next-agent/skills/` (wins a name clash) and `<vault>/skills/`. Discovery reads 4,000 chars, caches 30 s by folder mtimes, never follows symlinks. `skill_manage` batches (≤ 20 ops) snapshot every touched skill and roll back on the first failure; writes refuse symlinked components; `delete` archives to `.archive/` (never removes). Usage counters and pin/state live in `skill_usage`. The curator (weekly, or "Run curator now") marks agent-created, unpinned skills stale after 14 idle days and archives them after 30.

**Session store.** Each task is a session (`agent_sessions`, `agent_messages`, `agent_system_prompts`); `BuiltinExecutor::with_transcript` records messages, compaction marks the old ones inactive. FTS5 external-content index over a view that cuts tool output to 8,192 chars. `session_search`: discovery (1–10, default 3, 300 rows scanned, lineage dedupe, cron ranked last), scroll (±1..20), read (first 20 + last 10), browse (10). Search stays in the workspace and hides review sessions.

**Background review** (Settings toggle, default on): after a completed task with tool calls, a fork on the `summary` role runs as the `review` profile (only memory, skills, `read_file`, `search`), ≤ 16 turns and a token budget (¾ of the window, ≤ 600k; 120k unknown). It may add memory but not patch/remove, must `skill_view` before changing a skill, never deletes. It runs through the broker in manual mode with `ReviewApprover`, which answers only memory/skill writes, so each write is audited as approved by `background-review`. What it saved appears as a notice in the task's timeline.

**Cron** (`cron_jobs`): schedules `30m`, `every 2h`, `every monday 9am`, `in 30m`, 5-field cron, ISO time; IANA time zones (jiff). The scheduler ticks every 20 s while the app runs; `cron::tick` moves `next_run_at` by compare-and-swap before dispatch (at most once). Late runs within the catch-up window (half the period, 2 min..2 h, or `grace_minutes`) run once, older ones are recorded as `missed`. Each run is its own task (source `cron`, no memory tool, no review), with the job's skills in front of the prompt. Bypass is refused. Rust API: `AppService::{schedule_job, list_jobs, cancel_job, cron_save, cron_pause}` over `learning::cron::{schedule_job, list_jobs, update_job, set_paused, cancel_job, tick}`.

## Events
See [[Event Contracts]]. Every event persisted before being forwarded to the UI so the timeline is replayable after restart.
