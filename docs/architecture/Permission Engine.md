---
title: Permission Engine
tags: [architecture, security]
status: draft
---

# Permission Engine

Crate: `crates/permission-engine`. Pure function, no I/O, so it's fully table-testable:

```rust
fn evaluate(req: &ToolRequest, mode: Mode, rules: &RuleSet, ctx: &Context) -> Decision
enum Decision { Allow { reason }, Ask { reason, options }, Deny { reason } }
```
`Context` = canonical workspace root (or active worktree), home dir, sensitive path list, OS, task origin (ui/cli/channel).

## Request kinds
`Read(path)`, `Write(path)`, `Exec(command, cwd)`, `Network(url)`, `AskUser(question)`.

## Decision order (first match wins)
1. **User `Deny` rule** matches → `Deny` (all modes, incl. bypass).
2. **Catastrophic** command (e.g. `rm -rf /`, `rm -rf ~`, `mkfs`, `dd of=/dev/disk*`, fork bombs, `format C:`) → `Deny` (all modes).
3. **`AskUser`** → always pauses for a human (all modes).
4. **Mode policy** (table below).
5. **User `Allow` rule** can upgrade an `Ask` to `Allow` (never overrides 1–2; in `plan` never permits mutation).

## Mode policy
| Request | `plan` | `manual` | `auto` | `bypassPermissions` |
|---|---|---|---|---|
| Read inside workspace | Allow | Allow | Allow | Allow |
| Read outside workspace | Ask | Ask | Ask (sensitive paths: Deny) | Allow + audit |
| Write inside workspace | Deny | Ask | Allow (in worktree) | Allow |
| Write outside workspace | Deny | Ask | Ask (sensitive: Deny) | Allow + audit |
| Exec `safe_dev` | Deny | Ask | Allow | Allow |
| Exec `unknown` | Deny | Ask | Ask | Allow + audit |
| Exec `sensitive` / `network_remote` / `deploy` | Deny | Ask | Deny (blocked from auto-run, user may run manually) | Allow + audit |
| Network (agent tool) | Deny | Ask | Ask (allowlisted hosts: Allow) | Allow + audit |

`plan` may allow a whitelist of read-only commands (`git status`, `git log`, `ls`, `rg`) — decision tracked in [[Risks and Open Questions]].

## Path boundary checks
- Expand `~`, join relative paths to cwd, lexical normalization, then canonicalize the **nearest existing ancestor** (target may not exist yet) and re-append the remainder.
- Inside ⇔ canonical path starts with canonical root (component-wise, case-insensitive on Windows/macOS default FS).
- Symlinks/junctions resolving outside → outside. Use `dunce` on Windows to avoid `\\?\` mismatch.
- Sensitive list: `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config`, `~/.kube`, `~/.docker`, `~/Library/Keychains`, `/etc`, `/usr`, `/System`, `C:\Windows`, `%APPDATA%` credential stores, `.env*` files (Ask even inside workspace in `auto`).
- TOCTOU: broker re-checks and opens files via the checked canonical path.

## Command classification
- Tokenize with a POSIX-ish parser (`shell-words`/`conch-parser`); split compound commands on `|`, `&&`, `||`, `;`, newlines, and `$(…)`/backticks; **the most restrictive class of any segment wins**.
- `sh -c`, `bash -c`, `python -c`, `node -e`, `eval`, `xargs`, `sudo`, `env X=… cmd` → inspect inner command or classify `unknown`/`sensitive`.
- Classes:
  - `safe_dev`: `pnpm|npm|yarn|bun (test|lint|build|run <script>|install)`, `cargo (build|test|check|clippy|fmt)`, `git (status|diff|log|add|commit)` within worktree, `rg`, `ls`, `cat`, `tsc`, `vitest`, `pytest`, `uv run pytest`…
  - `sensitive`: `sudo`, `apt`, `brew`, `systemctl`, `launchctl`, `chmod -R`/`chown` outside ws, `crontab`, `defaults write`, `reg`.
  - `network_remote`: `ssh`, `scp`, `rsync` remote, `curl|wget` piped to shell, `ngrok`, `cloudflared`, `tailscale funnel`, `nc -l`.
  - `deploy`: `--prod`, `vercel deploy`, `fly deploy`, `kubectl apply`, `terraform apply`, `git push` (to protected branches / any push in auto).
  - `catastrophic`: see order step 2.
  - everything else: `unknown`.
- Rule patterns stored as prefix arrays (like `["pnpm","test"]`) or globs for paths, scoped `global | workspace`.

## Modes & transitions
- Default `manual`. `Shift+Tab` cycles `plan → manual → auto → plan`; never into bypass.
- Entering `bypassPermissions`: modal requiring typed confirmation, or CLI `--dangerously-skip-permissions`. Banner stays red while active. Channel-originated tasks can never enter it.
- Mode is per-session; changing mode mid-task applies to the next tool request.

## Audit
Every decision → `events`. Every bypass-allowed action, out-of-sandbox write, and approval → `audit_log` (append-only, hash-chained; see [[Data Model]]).

## Known limitation
M1 enforcement is at the tool-broker level. A permitted process (e.g. `pnpm test` running a malicious postinstall) can still act outside the workspace. M2 adds OS-level sandboxing; see [[ADR-0004 Permission Engine in Rust Core]].
