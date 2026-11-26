import { expect, test } from "@playwright/test";

for (const viewport of ["desktop", "375px"] as const) {
  test(`Chat leads to the project picker at ${viewport} width`, async ({ page }) => {
    if (viewport === "375px") await page.setViewportSize({ width: 375, height: 800 });
    await page.goto("/");

    const chat = page.getByRole("region", { name: "Chat", exact: true });
    const chooseProject = chat.getByRole("button", { name: "Choose a project", exact: true });
    const composer = chat.getByRole("textbox", { name: "Describe a task" });
    const menu = page.getByRole("button", { name: "Menu", exact: true, includeHidden: true });
    const sidebar = page.getByRole("navigation", { name: "Workspaces and tasks" });
    const openProject = sidebar.getByRole("button", { name: "Open Project", exact: true });

    await expect(chooseProject).toBeVisible();
    await expect(composer).toBeDisabled();
    await expect(menu).toHaveAttribute("aria-expanded", "false");
    await chooseProject.press("Enter");
    await expect(openProject).toBeFocused();
    await expect(menu).toHaveAttribute("aria-expanded", viewport === "375px" ? "true" : "false");
    await expect(sidebar.getByRole("button", { name: "New Project", exact: true })).toBeVisible();

    // The browser mock returns null from the folder picker, leaving manual entry available.
    await openProject.press("Enter");
    const path = sidebar.getByRole("textbox", { name: "Workspace folder path" });
    await expect(path).toBeFocused();
    await expect(path).toHaveValue("");
    const folder = "/projects/Existing Project";
    await path.fill(folder);

    if (viewport === "375px") {
      await path.press("Escape");
      await expect(menu).toHaveAttribute("aria-expanded", "false");
      await expect(menu).toBeFocused();
    }
    await chooseProject.press("Enter");
    await expect(path).toBeFocused();
    await expect(path).toHaveValue(folder);
    await path.press("Enter");

    const projects = sidebar.getByRole("list", { name: "Projects" });
    const activeProject = projects.getByRole("button", { name: "Existing Project", exact: true });
    await expect(activeProject).toHaveAttribute("aria-current", "true");
    await expect(path).toHaveCount(0);
    if (viewport === "375px") {
      await page.keyboard.press("Escape");
      await expect(menu).toHaveAttribute("aria-expanded", "false");
      await expect(menu).toBeFocused();
    }

    await expect(chat.getByText("Start a task", { exact: true })).toBeVisible();
    await expect(chooseProject).toHaveCount(0);
    await expect(composer).toBeEnabled();
  });
}
