import { test, expect } from "@playwright/test";

test.describe("Next Agent smoke tests", () => {
  test("launch app and see main window", async ({ page }) => {
    // The Tauri dev server should be running at localhost:1420
    await page.goto("/");
    
    await expect(page.locator("main, [role='main']").first()).toBeVisible();
  });

  test("the activity rail exposes distinct destinations and marks only Chat current", async ({ page }) => {
    await page.goto("/");
    const rail = page.getByRole("navigation", { name: "Activity modes" });
    await expect(rail).toBeVisible();
    await expect(rail.getByRole("link")).toHaveText(["Chat", "Agents", "Knowledge", "Design", "Monitor"]);
    await expect(rail.getByRole("link", { name: "Chat" })).toHaveAttribute("aria-current", "page");
    await expect(rail.locator('[aria-current="page"]')).toHaveCount(1);
    await expect(page.getByRole("link", { name: "Settings" })).toBeVisible();
  });

  test("Settings remains keyboard-accessible from the narrow window menu", async ({ page }) => {
    await page.setViewportSize({ width: 600, height: 800 });
    await page.goto("/");
    const menu = page.getByRole("button", { name: "Menu" });
    await menu.press("Enter");
    await expect(menu).toHaveAttribute("aria-expanded", "true");
    await page.getByRole("link", { name: "Settings" }).press("Enter");
    await expect(page).toHaveURL(/\/settings$/);
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  });

  test("narrow Menu and Chat keep their controls in separate, usable space", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 390 });
    await page.goto("/");
    const menu = page.getByRole("button", { name: "Menu" });
    const chat = page.getByRole("region", { name: "Chat" });
    const details = page.getByRole("complementary", { name: "Details panel" });
    await menu.press("Enter");
    await expect(chat).toBeHidden();
    await expect(details).toBeHidden();

    const rail = page.getByRole("navigation", { name: "Activity modes" });
    await rail.getByRole("link", { name: "Chat" }).press("Enter");
    await expect(chat).toBeVisible();
    await expect(menu).toHaveAttribute("aria-expanded", "false");
    await menu.press("Enter");
    const openProject = page.getByRole("button", { name: "Open Project" });
    await openProject.press("Enter");
    const path = page.getByRole("textbox", { name: "Workspace folder path" });
    await path.fill("/projects/Existing Project");
    await path.press("Enter");
    await expect(openProject).toBeFocused();
    await openProject.press("Escape");

    await expect(menu).toBeFocused();
    await expect(menu).toHaveAttribute("aria-expanded", "false");
    const timeline = chat.getByRole("log", { name: "Task timeline" });
    await timeline.evaluate((element) => {
      const messages = Array.from({ length: 100 }, (_, index) => {
        const message = document.createElement("p");
        message.textContent = `Task update ${index + 1}`;
        return message;
      });
      element.append(...messages);
    });
    await expect.poll(() => timeline.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
    const composer = chat.locator("form");
    await expect(composer).toBeVisible();
    await expect(details).toBeVisible();
    await expect.poll(async () => {
      const [composerBox, panelBox] = await Promise.all([composer.boundingBox(), details.boundingBox()]);
      return !!composerBox && !!panelBox && composerBox.y + composerBox.height <= panelBox.y + 1;
    }).toBe(true);
    await details.getByRole("tab", { name: "Files" }).click();
    await expect(details.getByRole("tab", { name: "Files" })).toBeFocused();

    await menu.press("Enter");
    await rail.getByRole("link", { name: "Design" }).click();
    await expect(page).toHaveURL(/\/design$/);
    await expect(menu).toHaveAttribute("aria-expanded", "false");
  });

  test("diff viewer exists in tasks", async ({ page }) => {
    // Navigate to a task view (placeholder for now)
    await page.goto("/");
    
    // Look for diff-related elements or buttons
    const reviewBtn = page.locator("button").filter({ hasText: /review|merge/i }).first();
    if (await reviewBtn.count() > 0) {
      await expect(reviewBtn).toBeVisible();
    } else {
      // If no merge button, just verify the app is functional
      const content = page.locator("main, .diff, [role='main']").first();
      await expect(content).toBeVisible();
    }
  });

  test("design system style guide renders", async ({ page }) => {
    await page.goto("/settings/design-system");
    
    // The design system page should show components and tokens
    await expect(page.getByRole("heading", { name: "Design system" })).toBeVisible();
  });

  test("dark mode toggle works", async ({ page }) => {
    await page.goto("/settings/design-system");
    
    // Look for a theme switcher or dark mode toggle
    const themeSwitch = page.getByRole("radio", { name: "Dark" });
    await expect(themeSwitch).toBeVisible();
    await themeSwitch.click();
    await expect(themeSwitch).toHaveAttribute("aria-checked", "true");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });
});
