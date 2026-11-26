import { expect, test } from "@playwright/test";
import type { DiffAction } from "../../src/lib/diff.ts";

test("kept reviews remain actionable after switching tasks", async ({ page }) => {
  await page.goto("/");
  const reviews = await page.evaluateHandle(async (modulePath) => {
    const { mockApi } = await import(modulePath) as typeof import("../../src/lib/mock-api.ts");
    const choices: { taskId: string; action: DiffAction }[] = [];
    let releaseMerge!: () => void;
    const mergePending = new Promise<void>((resolve) => { releaseMerge = resolve; });
    let nextTask = 0;
    mockApi.taskStart = async (_workspace, _prompt, _mode, deliver) => {
      const number = ++nextTask;
      const taskId = `tsk_review_${number}`;
      deliver({
        type: "review",
        task_id: taskId,
        branch: `next-agent/mock-${number}`,
        files: [{ path: `review-${number}.md`, status: "added", hunks: [] }],
      });
      deliver({
        type: "event",
        event: {
          v: 1,
          id: `${taskId}:0`,
          task_id: taskId,
          seq: 0,
          ts: new Date().toISOString(),
          kind: "task_completed",
          data: { summary: "Review ready" },
        },
      });
      return taskId;
    };
    mockApi.reviewAction = async (taskId, action) => {
      choices.push({ taskId, action });
      if (action === "merge") {
        await mergePending;
        return { action, commit: "a1b2c3d4e5f6", files: [], notice: "The task record could not be updated." };
      }
      return { action, commit: null, files: [] };
    };
    return { choices, releaseMerge };
  }, "/src/lib/mock-api.ts");

  const sidebar = page.getByRole("navigation", { name: "Workspaces and tasks" });
  await sidebar.getByRole("button", { name: "Open Project", exact: true }).click();
  const path = sidebar.getByRole("textbox", { name: "Workspace folder path" });
  await path.fill("/projects/Existing Project");
  await path.press("Enter");
  const draft = page.getByRole("textbox", { name: "Describe a task" });
  const panel = page.getByRole("complementary", { name: "Details panel" });
  const reviewer = panel.getByRole("region", { name: "Diff reviewer" });
  const actions = reviewer.getByRole("group", { name: "Review actions" });
  const merge = actions.getByRole("button", { name: "Merge", exact: true });
  const discard = actions.getByRole("button", { name: "Discard", exact: true });
  const status = reviewer.getByRole("status");
  const tasks = sidebar.getByRole("list", { name: "Tasks" });
  const mergeTask = tasks.getByRole("button", { name: /Review the project notes/ });
  const discardTask = tasks.getByRole("button", { name: /Review the draft outline/ });

  await draft.fill("Review the project notes");
  await draft.press("Enter");
  await expect(reviewer).toContainText("review-1.md");
  await actions.getByRole("button", { name: "Keep for later" }).press("Enter");
  await expect(status).toHaveText("Changes kept for later.");

  await draft.fill("Review the draft outline");
  await draft.press("Enter");
  await expect(reviewer).toContainText("review-2.md");
  await expect(status).toHaveCount(0);
  await actions.getByRole("button", { name: "Keep for later" }).press("Enter");
  await expect(status).toHaveText("Changes kept for later.");

  await mergeTask.press("Enter");
  await expect(mergeTask).toHaveAttribute("aria-current", "true");
  await expect(reviewer).toContainText("review-1.md");
  await expect(status).toHaveText("Changes kept for later.");
  await expect(merge).toBeEnabled();
  await expect(discard).toBeEnabled();
  await merge.press("Enter");
  await expect(merge).toBeDisabled();
  await panel.getByRole("tab", { name: "Files", exact: true }).click();
  await expect(reviewer).toHaveCount(0);
  await panel.getByRole("tab", { name: "Diff", exact: true }).click();
  await expect(merge).toBeDisabled();
  await expect(discard).toBeDisabled();
  expect(await reviews.evaluate(({ choices }) => choices.filter(({ action }) => action === "merge"))).toEqual([
    { taskId: "tsk_review_1", action: "merge" },
  ]);

  await reviews.evaluate(({ releaseMerge }) => releaseMerge());
  await expect(status).toHaveText("Changes merged (a1b2c3d4).");
  const timeline = page.getByRole("log", { name: "Task timeline" });
  await expect(timeline.getByText("The task record could not be updated.")).toBeVisible();
  await expect(actions).toHaveCount(0);

  await discardTask.press("Enter");
  await expect(discardTask).toHaveAttribute("aria-current", "true");
  await expect(reviewer).toContainText("review-2.md");
  await expect(status).toHaveText("Changes kept for later.");
  await discard.press("Enter");
  await expect(status).toHaveText("Changes discarded.");
  await expect(actions).toHaveCount(0);
  expect(await reviews.evaluate(({ choices }) => choices)).toEqual([
    { taskId: "tsk_review_1", action: "keep" },
    { taskId: "tsk_review_2", action: "keep" },
    { taskId: "tsk_review_1", action: "merge" },
    { taskId: "tsk_review_2", action: "discard" },
  ]);
});
