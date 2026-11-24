import { expect, test } from "@playwright/test";

test("opening a folder discovers project files and opens its vault graph", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open Project", exact: true }).click();
  const path = page.getByRole("textbox", { name: "Workspace folder path" });
  await expect(path).toBeFocused();
  await path.fill("/projects/Existing Project");
  await page.getByRole("button", { name: "Open", exact: true }).click();

  const projects = page.getByRole("list", { name: "Projects" });
  await expect(projects.getByRole("button", { name: "Existing Project" })).toHaveAttribute("aria-current", "true");
  const connections = page.getByRole("region", { name: "Project connections" });
  await expect(connections.locator("dl").getByText("AGENTS.md", { exact: true })).toBeVisible();
  await expect(connections.locator("dl").getByText("DESIGN.md", { exact: true })).toBeVisible();
  await expect(connections.locator("dl").getByText("tokens.css", { exact: true })).toBeVisible();
  await expect(connections.getByText("/projects/Existing Project/vault", { exact: true })).toBeVisible();
  await connections.getByText("Project file tree", { exact: true }).click();
  await connections.getByText("src/", { exact: true }).click();
  await expect(connections.getByText("main.ts")).toBeVisible();
  await connections.getByRole("link", { name: "Open vault graph" }).click();
  await expect(page).toHaveURL(/\/notes\?view=graph$/);
  await expect(page.getByRole("radio", { name: "Graph" })).toHaveAttribute("aria-checked", "true");
});

test("creating a project selects its empty tree and switching refreshes its connections", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open Project", exact: true }).click();
  await page.getByRole("textbox", { name: "Workspace folder path" }).fill("/projects/Existing Project");
  await page.getByRole("button", { name: "Open", exact: true }).click();

  const connections = page.getByRole("region", { name: "Project connections" });
  await expect(connections.locator("dl").getByText("AGENTS.md", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "New Project", exact: true }).click();
  await page.getByRole("textbox", { name: "Project name" }).fill("New Workspace");
  await expect(page.getByRole("button", { name: "Create project", exact: true })).toBeDisabled();
  await page.getByRole("textbox", { name: "Parent folder path" }).fill("/projects");
  await page.getByRole("button", { name: "Create project", exact: true }).click();

  const projects = page.getByRole("list", { name: "Projects" });
  await expect(projects.getByRole("button", { name: "New Workspace" })).toHaveAttribute("aria-current", "true");
  await expect(connections.getByRole("link", { name: "Open vault graph" })).toHaveCount(0);
  await connections.getByText("Project file tree", { exact: true }).click();
  await expect(connections.getByText("No files yet")).toBeVisible();
  await projects.getByRole("button", { name: "Existing Project" }).click();
  await expect(connections.locator("dl").getByText("AGENTS.md", { exact: true })).toBeVisible();
  await expect(connections.getByRole("link", { name: "Open vault graph" })).toBeVisible();
});
