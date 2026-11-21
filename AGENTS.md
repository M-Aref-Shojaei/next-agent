# Agent Workflow

These rules apply to every task in this repository.

## Required task flow

1. **Plan**
   - Restate requested outcome.
   - Inspect project documentation, task tracker, architecture, conventions, and relevant files.
   - Define acceptance criteria, risks, dependencies, and validation commands.
   - Register task in the existing task table before implementation.

2. **Decompose and delegate**
   - Break work into small, independently verifiable subtasks.
   - Assess complexity and dispatch specialist agents when useful:
     - Research/codebase audit
     - Architecture/design
     - Implementation
     - Test design/execution
     - Security and independent review
   - Run independent subtasks in isolated worktrees when they edit overlapping or shared files.
   - Never allow agents to silently overwrite user work or another agent's changes.
   - Use current repository tools, GitHub features, available skills, MCP integrations, plugins, and installed dependencies when they improve correctness or reduce work. Do not add dependencies without need.

3. **Read before writing**
   - Read existing implementation, tests, docs, configuration, and APIs before editing.
   - Reuse existing design-system tokens/components, helpers, dependencies, and project patterns.
   - Do not invent APIs, provider state, integrations, or capabilities.

4. **Implement**
   - Make the smallest correct change.
   - Preserve compatibility unless task explicitly requires a breaking change.
   - Keep security, privacy, accessibility, error handling, and input validation intact.

5. **Test**
   - Write or update focused tests for every non-trivial behavior.
   - Record expected behavior before running tests.
   - Run focused tests first, then the project's clean-code checks, formatter/linter/type checker, build, and full test suite.
   - If a check fails, diagnose and fix before continuing.

6. **Clean and retest**
   - Run formatting, linting, type checks, static analysis, design-token checks, and dependency/lockfile checks required by the project.
   - Remove generated artifacts and temporary files from the change unless intentionally tracked.
   - Run the relevant tests again after cleanup and structural changes.

7. **Review**
   - Perform independent review, preferably through a reviewer agent or isolated review pass.
   - Check correctness, security, regressions, accessibility, documentation, scope, and test evidence.
   - Reject incomplete work and return to the needed earlier stage.

8. **Deliver for approval**
   - Create a focused Git checkpoint/commit.
   - Push a branch and open a GitHub pull request when repository credentials and remote access permit.
   - Never merge the PR without user approval.
   - Report PR URL, changed files, tests written, commands and results, review findings, risks, and approval decision needed.
   - If push/PR creation is unavailable, stop after a local commit and report the exact blocker; do not claim PR completion.

## Quality gates

- No implementation before plan and file inspection.
- No test claim without command output or recorded evidence.
- No completion without independent review.
- No release before required CI/CD succeeds.
- User approval is required before merging approved/rejected review work.

## Task tracking

Use the repository task tracker under `docs/plan/` when present. Add or update:

- Task status and acceptance criteria.
- Test commands and evidence.
- UI change approval rows in `docs/plan/UiTestResults.md`.
- Remaining risks and blockers.

## Communication

Start each progress report with a concise summary. Keep plans and reports concrete. State blockers, decisions, risks, and one next action. Do not claim an agent completed work until its files and test evidence are reviewed by the orchestrator.
