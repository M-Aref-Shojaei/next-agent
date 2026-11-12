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

## Tool set (M1)
`read_file`, `list_dir`, `search`, `write_file`, `apply_patch`, `run_command` (streamed stdout/stderr, timeout, kill on cancel), `http_fetch`, `ask_user`.

## Events
See [[Event Contracts]]. Every event persisted before being forwarded to the UI so the timeline is replayable after restart.
