# Branch and PR Rules

Read this document before creating or updating a pull request.

## Normal development branches

Use `feat/*`, `fix/*`, `refactor/*`, or `security/*` for feature and bug work.

- Branch from `main`.
- Run full shared validation on every push and pull request.
- Required shared workflows: `.github/workflows/ci.yml` and `.github/workflows/ui.yml`.
- Full validation covers macOS, Windows, Linux, Rust, TypeScript, and UI tests.
- Do not publish release assets from normal development branches.

## Platform release branches

Use one branch per platform:

- `release/macos-*`
- `release/windows-*`
- `release/linux-*`

Rules:

- Branch from a commit that already passed full shared validation.
- Include only platform-specific fixes and release work.
- Skip shared full-matrix workflows on release branches.
- Run only matching platform validation and release build:
  - macOS: `macos-latest`
  - Windows: `windows-latest`
  - Linux: `ubuntu-22.04`
- Run platform validation on release pull requests and manual workflow runs.
- Publish assets only from approved `v*` tag runs.
- Never merge or publish while required checks fail or remain pending.

## Before opening a pull request

Verify:

1. Branch naming and base branch.
2. Changed files match branch purpose.
3. Workflow trigger and runner platform.
4. Release publish condition is tag-only.
5. `gh pr checks <number>` shows expected checks only.
