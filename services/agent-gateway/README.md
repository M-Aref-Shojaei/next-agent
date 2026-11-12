# agent-gateway

TypeScript adapter that runs external coding-agent engines (opencode first) behind
the `AgentExecutor` contract. Tools are **not** executed inside the engine: its
tool calls are forwarded to the Rust tool broker, which applies the permission
engine.

Status: stub (M2). See `docs/architecture/Agent Execution.md`.
