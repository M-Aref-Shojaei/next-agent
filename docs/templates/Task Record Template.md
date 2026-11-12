---
title: Task Record Template
tags: [template]
---

# Task Record Template

Format written by the [[Vault Documentation Service]] for each completed task.

```markdown
---
type: task
id: tsk_01J…
title: Add login form validation
workspace: my-app
mode: auto
status: completed        # completed | failed | cancelled
branch: next-agent/auto-3f2a
base: a1b2c3d
merged: false            # updated on merge/discard
model: <model-id>
started: 2026-11-10T10:02:11+03:30
finished: 2026-11-10T10:09:40+03:30
tags: [next-agent/task]
---

# Add login form validation

## Request
> original user prompt

## Summary
LLM-written 2–5 sentence summary.

## Changes
| File | +/− |
|---|---|
| src/login.ts | +42 −3 |

## Commands run
- `pnpm test` — ok (exit 0)

## Permission decisions
- Allowed (auto): `pnpm test`
- Asked → Allowed by user: `curl https://api.example.com`

## Follow-ups
- [ ] …

<!-- next-agent:end -->
User notes below this line are preserved.
```
