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

## Docs

- [Roadmap](docs/plan/Roadmap.md) — milestones M0–M5
- [M1 Vertical Slice](docs/plan/M1%20Vertical%20Slice.md) — first milestone task breakdown
- [M1 Progress](docs/plan/M1%20Progress.md) — live task ledger
- [Home](docs/Home.md) — documentation vault entry point

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
