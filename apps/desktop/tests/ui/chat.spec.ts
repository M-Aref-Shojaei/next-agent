import { expect, test } from "@playwright/test";

test("Chat keeps a multiline draft on failure and preserves edits during a retry", async ({ page }) => {
  await page.goto("/");
  const start = await page.evaluateHandle(async (modulePath) => {
    const { mockApi } = await import(modulePath) as typeof import("../../src/lib/mock-api.ts");
    const prompts: string[] = [];
    let releaseRetry!: () => void;
    const retryPending = new Promise<void>((resolve) => { releaseRetry = resolve; });
    mockApi.taskStart = async (_workspace, prompt) => {
      const attempt = prompts.push(prompt);
      if (attempt === 1) throw new Error("Task could not start. Try again.");
      if (attempt === 2) await retryPending;
      return `tsk_draft_${attempt}`;
    };
    return { prompts, releaseRetry };
  }, "/src/lib/mock-api.ts");

  const sidebar = page.getByRole("navigation", { name: "Workspaces and tasks" });
  await sidebar.getByRole("button", { name: "Open Project", exact: true }).click();
  const path = sidebar.getByRole("textbox", { name: "Workspace folder path" });
  await path.fill("/projects/Existing Project");
  await path.press("Enter");

  const chat = page.getByRole("region", { name: "Chat" });
  const draft = chat.getByRole("textbox", { name: "Describe a task" });
  const send = chat.getByRole("button", { name: "Send", exact: true });
  const starting = chat.getByRole("button", { name: "Starting…", exact: true });
  const alert = page.getByRole("main").getByRole("alert");
  const tasks = sidebar.getByRole("list", { name: "Tasks" }).getByRole("button");
  const submitted = "Read the project notes.\n  Preserve this indentation.";
  const edited = "Summarize the open questions.\nInclude the remaining decisions.";

  await draft.fill("Read the project notes.");
  await draft.press("End");
  await draft.press("Shift+Enter");
  await draft.pressSequentially("  Preserve this indentation.");
  await expect(draft).toHaveValue(submitted);
  await draft.dispatchEvent("keydown", { key: "Enter", isComposing: true });
  await draft.dispatchEvent("keydown", { key: "Enter", keyCode: 229 });
  expect(await start.evaluate(({ prompts }) => prompts)).toEqual([]);
  await draft.press("Enter");
  await expect(alert).toContainText("Task could not start. Try again.");
  await expect(draft).toHaveValue(submitted);
  await expect(tasks).toHaveCount(0);

  await send.click();
  await expect(starting).toBeDisabled();
  await expect(draft).toHaveValue(submitted);
  await draft.press("Enter");
  await draft.fill(edited);
  await expect(starting).toBeDisabled();
  expect(await start.evaluate(({ prompts }) => prompts)).toEqual([submitted, submitted]);

  await start.evaluate(({ releaseRetry }) => releaseRetry());
  await expect(tasks).toHaveCount(1);
  await expect(tasks.first()).toContainText("Read the project notes.");
  await expect(alert).toHaveCount(0);
  await expect(draft).toHaveValue(edited);

  await send.click();
  await expect(tasks).toHaveCount(2);
  await expect(draft).toHaveValue("");
  await expect(send).toBeDisabled();
  expect(await start.evaluate(({ prompts }) => prompts)).toEqual([submitted, submitted, edited]);
});
