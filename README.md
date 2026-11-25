<p align="center"><img src="apps/desktop/src-tauri/icons/128x128@2x.png" width="128" alt="Next Agent logo"></p>

# Next Agent

**Local-first, cross-platform AI workstation:** coding agents + Markdown knowledge vault + auditable permission engine, built on Tauri 2 (Rust + SvelteKit).

Everything runs on your machine. Agents propose changes, you review and approve them in side-by-side diffs, and every decision is written to a tamper-evident audit log. Your prompts, notes, and files never leave your computer.

## Quick start

### 1. Prerequisites

| Tool | Version | Why |
|------|---------|-----|
| Node.js | `>= 22.6` | Frontend + test runner (`node scripts/test.mjs`) |
| pnpm | `10` | Package manager (`corepack enable` then `pnpm install`) |
| Rust | stable | Tauri core + Rust crates (`cargo test`, `cargo build`) |

Install Rust: https://rustup.rs/  
Install pnpm: `corepack enable && corepack prepare pnpm@10 --activate`

### 2. Clone and install

```sh
git clone https://github.com/M-Aref-Shojaei/next-agent.git
cd next-agent
pnpm install
```

### 3. Run the test suites (no Rust needed)

The TypeScript suites run with plain Node — zero dependencies:

```sh
node scripts/test.mjs        # TS tests
```

Rust tests (requires the Rust toolchain):

```sh
cargo test --workspace
```

Full checks (TypeScript + Rust structure + design tokens + log fields):

```sh
make check
```

### 4. Run the desktop app

```sh
# Dev mode (Tauri)
cargo tauri dev   # or: pnpm tauri dev if wired

# Production build
cargo tauri build
```

The app opens a local window. No external accounts or API keys required to start.

## What's inside

- `crates/` — Rust core: workspace, SQLite store, permission engine, agent executor, git worktrees, vault writer, diagnostics log
- `apps/desktop/` — Tauri 2 shell (Rust) + SvelteKit frontend
- `packages/contracts` — versioned event contracts shared by Rust and TypeScript
- `packages/vault` — Markdown frontmatter, `[[wikilinks]]`, slugs, task-record paths
- `docs/` — self-contained Markdown vault (Obsidian/Logseq compatible). Open `docs/` as a vault to browse it.

## Project task table

| Task | Required outcome |
|---|---|
| PS-01 — Open or create project instructions | Read the project's `AGENTS.md` on Open and New Project. Scaffold a starter file for a new project and offer to create one when an existing folder has none. Preserve any existing instructions. |
| CL-03 — Open or attach the project vault | Discover an existing vault or let the user select one. Load its documents and graph into the active project, and offer a local vault setup when none exists. |
| PT-01 — Show vault-backed tasks | Store the to-do list and task table as Markdown in the project's vault. Show the same records in a dedicated Tasks tab, reachable from the project sidebar. |
| UX-01, UX-02 — Refactor desktop UI and UX | Audit the desktop experience, then simplify project opening, navigation, Tasks, Design, and code review with the existing design system. |
| 4, SEC-01 — Review security boundaries | Verify existing permissions and worktree review. Audit project isolation and diagnostics, then track confirmed gaps as separate fixes. |
| PERF-01, PERF-02 — Measure and improve performance | Establish native discovery and vault indexing baselines, then optimize one measured bottleneck if the data supports a change. |
| RF-01, RF-02 — Refactor duplicate code | Audit overlapping implementations, choose the canonical path for each, then retire redundant code in focused changes with regression tests. |

The [vault Task Tracker](docs/plan/Task%20Tracker.md) records status, dependencies, tests, and approval for these tasks. Its [execution table](docs/plan/Task%20Tracker.md#execution-table) lists the smaller tasks an individual contributor or agent can claim and complete. The [Parallel Refactor Plan](docs/plan/Parallel%20Refactor%20Plan.md) shows where agents can work concurrently. The [Product Work Plan](docs/plan/Product%20Work%20Plan.md) describes how the workflows fit together.

## Docs

- [Roadmap](docs/plan/Roadmap.md) — milestones M0–M5
- [M1 Vertical Slice](docs/plan/M1%20Vertical%20Slice.md) — first milestone task breakdown
- [M1 Progress](docs/plan/M1%20Progress.md) — live task ledger
- [Home](docs/Home.md) — documentation vault entry point

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
