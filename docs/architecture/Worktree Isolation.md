---
title: Worktree Isolation
tags: [architecture, git]
status: draft
---

# Worktree Isolation

Any task that mutates the repo in `auto` mode works in an isolated Git worktree. See [[ADR-0005 Git Worktrees for Auto Mode]].

## Lifecycle
1. **Create** lazily on the first mutating tool request:
   `git worktree add <ws>/.next-agent/worktrees/<task-id> -b next-agent/auto-<task-short-id> <base>` where `<base>` = current HEAD of the main tree (recorded on the task).
   `.next-agent/` is added to `.git/info/exclude` (not `.gitignore`, to avoid touching user files).
2. **Run**: the task's sandbox root and default cwd become the worktree path. Reads of the main tree are still allowed (inside workspace).
3. **Checkpoint**: the agent (or broker at task end) commits to the auto branch with a generated message.
4. **Review**: UI shows `git diff <base>...next-agent/auto-*` side-by-side, per file, plus commit list.
5. **Resolve**:
   - *Merge* → into the branch checked out in the main tree (squash or merge commit, see Q5). Refuse if main tree is dirty in conflicting files; show conflicts.
   - *Discard* → `git worktree remove --force` + `git branch -D`.
   - *Keep* → leave branch for later; listed in sidebar.
6. **Cleanup**: `git worktree prune` on startup; orphaned worktrees listed in Settings.

## Edge cases
- Non-git workspace → auto mode unavailable (fall back to manual), see Q4.
- Submodules / LFS → `git worktree` handles; LFS smudge may be slow — show progress.
- Base moved during task → diff stays against recorded base; merge does a normal three-way merge.
- `manual` and `bypass` modes edit the main tree directly (user is watching / opted out) — optional per-workspace setting to always use worktrees.

## Implementation
Shell out to the system `git` (most faithful worktree support) via the process layer, not through the agent tool path. `git2`/`gix` for read-only status/diff where convenient.
