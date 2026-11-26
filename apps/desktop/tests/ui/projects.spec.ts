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
  await expect(projects.getByRole("button", { name: "Existing Project" })).toHaveAttribute("aria-current", "true");
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

for (const viewport of ["desktop", "narrow"] as const) {
  test(`project forms restore focus after cancellation and recover from a duplicate name at ${viewport} width`, async ({ page }) => {
    if (viewport === "narrow") await page.setViewportSize({ width: 600, height: 800 });
    await page.goto("/");
    if (viewport === "narrow") {
      const menu = page.getByRole("button", { name: "Menu" });
      await menu.press("Enter");
      await expect(menu).toHaveAttribute("aria-expanded", "true");
    }

    const sidebar = page.getByRole("navigation", { name: "Workspaces and tasks" });
    const openProject = sidebar.getByRole("button", { name: "Open Project", exact: true });
    const path = sidebar.getByRole("textbox", { name: "Workspace folder path" });
    await openProject.press("Enter");
    await expect(path).toBeFocused();
    await expect(path).toHaveValue("");
    await sidebar.getByRole("button", { name: "Cancel", exact: true }).press("Enter");
    await expect(path).toHaveCount(0);
    await expect(openProject).toBeFocused();

    await openProject.press("Enter");
    await path.fill("/projects/Existing Project");
    await path.press("Enter");
    const projects = sidebar.getByRole("list", { name: "Projects" });
    await expect(projects.getByRole("button", { name: "Existing Project" })).toHaveAttribute("aria-current", "true");

    const newProject = sidebar.getByRole("button", { name: "New Project", exact: true });
    const projectName = sidebar.getByRole("textbox", { name: "Project name" });
    const parentPath = sidebar.getByRole("textbox", { name: "Parent folder path" });
    const createForm = sidebar.locator("form").filter({ has: page.getByRole("textbox", { name: "Project name" }) });
    await newProject.press("Enter");
    await expect(projectName).toBeFocused();
    await projectName.fill("Existing Project");
    await parentPath.fill("/projects");
    await parentPath.press("Enter");

    await expect(createForm.getByRole("alert")).toHaveText("Project folder already exists.");
    await expect(projectName).toHaveValue("Existing Project");
    await expect(parentPath).toHaveValue("/projects");
    await expect(projectName).toBeFocused();

    await projectName.fill("Recovered Project");
    await projectName.press("Enter");
    await expect(createForm).toHaveCount(0);
    await expect(projects.getByRole("button", { name: "Recovered Project" })).toHaveAttribute("aria-current", "true");
    await expect(newProject).toBeFocused();
  });
}

test("completing a project creation keeps focus outside the form when the user moved it", async ({ page }) => {
  await page.goto("/");
  const releaseCreation = await page.evaluateHandle(async (modulePath) => {
    const { mockApi } = await import(modulePath) as typeof import("../../src/lib/mock-api.ts");
    const createWorkspace = mockApi.workspaceCreate;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    mockApi.workspaceCreate = async (parent, name) => {
      await pending;
      return createWorkspace(parent, name);
    };
    return release;
  }, "/src/lib/mock-api.ts");

  const sidebar = page.getByRole("navigation", { name: "Workspaces and tasks" });
  const newProject = sidebar.getByRole("button", { name: "New Project", exact: true });
  const form = sidebar.getByRole("form", { name: "New Project" });
  await newProject.press("Enter");
  await form.getByRole("textbox", { name: "Project name" }).fill("Deferred Project");
  await form.getByRole("textbox", { name: "Parent folder path" }).fill("/projects");
  await form.getByRole("button", { name: "Create project", exact: true }).press("Enter");
  await expect(form).toHaveAttribute("aria-busy", "true");
  await expect(sidebar.getByRole("status")).toHaveText("Creating project…");

  const settings = sidebar.getByRole("link", { name: "Settings" });
  await settings.focus();
  await expect(settings).toBeFocused();
  await releaseCreation.evaluate((release) => release());

  const projects = sidebar.getByRole("list", { name: "Projects" });
  await expect(projects.getByRole("button", { name: "Deferred Project" })).toHaveAttribute("aria-current", "true");
  await expect(form).toHaveCount(0);
  await expect(newProject).toBeVisible();
  await expect(settings).toBeFocused();
});
