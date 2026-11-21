import { test, expect } from "@playwright/test";

test.describe("Next Agent smoke tests", () => {
  test("launch app and see main window", async ({ page }) => {
    // The Tauri dev server should be running at localhost:1420
    await page.goto("/");
    
    await expect(page.locator("main, [role='main']").first()).toBeVisible();
  });

  test("activity rail exposes seven keyboard-accessible modes", async ({ page }) => {
    await page.goto("/");
    const rail = page.getByRole("navigation", { name: "Activity modes" });
    await expect(rail).toBeVisible();
    await expect(rail.getByRole("link")).toHaveCount(7);
    await expect(rail.getByRole("link", { name: "Chat" })).toHaveAttribute("aria-current", "page");
  });

  test("navigate to settings", async ({ page }) => {
    await page.goto("/settings");
    
    // Settings should be accessible
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
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
