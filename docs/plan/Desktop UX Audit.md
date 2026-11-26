# Desktop UX audit

Baseline `d4a89cc`, the merge of PR #11. Chromium covers project open and create, retry, recent-project switching within a session, vault graph navigation, and keyboard navigation. The narrow captures use a 375px dark window. The Design and diff-review findings below are source walkthroughs.

| Priority | Reproduction or walkthrough | Expected behavior and verification | Owner |
|---|---|---|---|
| High | Open a blank folder or create a project. Connections shows “None found” for agent instructions. | Existing instructions stay readable. New Project scaffolds a starter. Open offers explicit creation if the file is missing. Verify the existing file stays unchanged and a quick project switch keeps the action scoped. | PS-01 |
| High | Open a project without a recognized vault and switch to Notes. The Notes setup uses the global vault setting. | Attach or initialize a vault for the selected project and show where records will be saved. Verify separate destinations for two projects and recovery after a vault disappears. | CL-03 |
| High | Reject a Chat task start. The current form clears the prompt before the start result returns. | Keep the multiline draft on failure and let the user retry. Verify one accepted task and no overwritten edits if the user types while starting. | UX-02-G |
| High | Keep a worktree for later. The task retains the worktree while review controls disappear. | Returning to the kept task makes Merge and Discard available. Verify both decisions through separate review fixtures. | UX-02-B |
| Medium | At 375px with no project, Chat prompts users to open one while the actions sit behind Menu. With a project selected, the empty timeline still asks users to open a workspace. | Give first-time users a clear route to the project actions and show the next step after selection. Verify both states using the keyboard. | UX-02-A |
| Medium | Open two folders with the same basename. Project rows differ only in a hover tooltip. | Show enough parent path to distinguish the two entries at both widths. Verify that selecting each entry loads the matching connections. | UX-02-A |
| Medium | In Design, generate a task and follow Review diff. Copy refers to Tasks while the action opens Chat. | Name the actual destination and preserve the selected task and available code actions after keyboard activation. Verify the Design-to-Chat handoff. | UX-02-B, CL-02 |
| Planned flow | Project Tasks will be a new destination. The sidebar currently lists agent runs. | Open Tasks from the project sidebar, handle a missing vault, quick-add a record, and reopen two independent task lists offline. Verify planned records and agent-run history are recognizable. | PT-01, PT-02 |

The narrow shell implementation keeps Menu and Chat in a shared content row with one visible at a time. The browser regression checks a 375 × 390 viewport, current-route rail activation, Escape focus restoration, a scrollable long timeline, and the review-panel boundary. Native folder-picker behavior is tracked in UX-02-F and requires interactive macOS verification.
