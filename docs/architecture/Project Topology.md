---
title: Project Topology
tags: [architecture, generated]
---

# Project Topology

How the separate projects in this repo connect. **Generated** by `node scripts/gen-topology.mjs` (`make topology`) from the Cargo and package manifests plus a few probed cross-language links; `make check` fails if this note is stale. Don't edit by hand.

See [[Architecture Overview]] for what each box does and [[Module Boundaries]] for the rules behind the arrows.

```mermaid
flowchart LR
  subgraph app["Desktop app"]
    direction TB
    next_agent_desktop["desktop backend (Tauri)<br/><small>apps/desktop/src-tauri</small>"]
    ts_desktop["desktop UI<br/><small>apps/desktop</small>"]
  end
  subgraph rust["Rust crates"]
    direction TB
    next_agent_core["core<br/><small>crates/core</small>"]
    next_agent_permission_engine["permission-engine<br/><small>crates/permission-engine</small>"]
    next_agent_process_manager["process-manager<br/><small>crates/process-manager</small>"]
  end
  subgraph ts["TypeScript packages"]
    direction TB
    ts_contracts["contracts<br/><small>packages/contracts</small>"]
    ts_policy["policy<br/><small>packages/policy</small>"]
    ts_vault["vault<br/><small>packages/vault</small>"]
  end
  subgraph planned["Planned (no code yet)"]
    direction TB
    ui["ui<br/><small>packages/ui</small>"]
    agent_gateway["agent-gateway<br/><small>services/agent-gateway</small>"]
    channel_gateway["channel-gateway<br/><small>services/channel-gateway</small>"]
    hermes_adapter["hermes-adapter<br/><small>services/hermes-adapter</small>"]
    router_adapter["router-adapter<br/><small>services/router-adapter</small>"]
  end
  subgraph external["Outside the repo"]
    direction TB
    ext_9router[("9Router (user-run LLM router)")]
    ext_git[("git")]
    ext_vault[("Markdown vault folder")]
  end
  next_agent_core --> next_agent_permission_engine
  next_agent_core --> next_agent_process_manager
  next_agent_desktop --> next_agent_core
  next_agent_desktop --> next_agent_permission_engine
  next_agent_desktop --> next_agent_process_manager
  ts_desktop --> ts_contracts
  ts_desktop ==>|"IPC: invoke / Channel"| next_agent_desktop
  ts_policy <-.->|"rule parity (check-policy-parity)"| next_agent_permission_engine
  ts_contracts <-.->|"event envelope v1"| next_agent_core
  ts_vault <-.->|"shared vault fixtures"| next_agent_core
  next_agent_core -->|"HTTP :20128/v1"| ext_9router
  next_agent_core -->|"git CLI (worktrees)"| ext_git
  next_agent_core -->|"task records (.md)"| ext_vault
  classDef rust fill:#f6e3d4,stroke:#b7410e,color:#3b1a05
  classDef ts fill:#dbe9fb,stroke:#3178c6,color:#0b2545
  classDef planned fill:#eeeeee,stroke:#999,stroke-dasharray:4 3,color:#555
  classDef ext fill:#e8f5e9,stroke:#2e7d32,color:#1b3d1f
  class next_agent_core rust
  class next_agent_permission_engine rust
  class next_agent_process_manager rust
  class next_agent_desktop rust
  class ts_desktop ts
  class ts_contracts ts
  class ts_policy ts
  class ui planned
  class ts_vault ts
  class agent_gateway planned
  class channel_gateway planned
  class hermes_adapter planned
  class router_adapter planned
  class ext_9router ext
  class ext_git ext
  class ext_vault ext
```

**Legend:** solid arrow = depends on (manifest) · dotted `dev` = test-only dependency · thick arrow = runtime IPC · dotted two-way = kept in sync across languages (parity checks / shared fixtures) · green = outside the repo · grey dashed = planned, README only.

| Project | Kind | Path | Depends on | Used by | Mirrors |
|---|---|---|---|---|---|
| core | Rust crate | `crates/core` | permission-engine, process-manager | desktop backend (Tauri) | contracts, vault |
| permission-engine | Rust crate | `crates/permission-engine` | — | core, desktop backend (Tauri) | policy |
| process-manager | Rust crate | `crates/process-manager` | — | core, desktop backend (Tauri) | — |
| desktop backend (Tauri) | Rust crate | `apps/desktop/src-tauri` | core, permission-engine, process-manager | desktop UI | — |
| desktop UI | TS package | `apps/desktop` | contracts, desktop backend (Tauri) | — | — |
| contracts | TS package | `packages/contracts` | — | desktop UI | core |
| policy | TS package | `packages/policy` | — | — | permission-engine |
| ui | planned (stub) | `packages/ui` | — | — | — |
| vault | TS package | `packages/vault` | — | — | core |
| agent-gateway | planned (stub) | `services/agent-gateway` | — | — | — |
| channel-gateway | planned (stub) | `services/channel-gateway` | — | — | — |
| hermes-adapter | planned (stub) | `services/hermes-adapter` | — | — | — |
| router-adapter | planned (stub) | `services/router-adapter` | — | — | — |
