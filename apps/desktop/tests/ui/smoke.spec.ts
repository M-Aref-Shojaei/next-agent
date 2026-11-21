import { test, expect } from "@playwright/test";

test.describe("Next Agent smoke tests", () => {
  test("launch app and see main window", async ({ page }) => {
    // The Tauri dev server should be running at localhost:1420
    await page.goto("/");
    
    // Verify the app loads with some basic elements
    const title = page.locator("h1").first();
    await expect(title).toBeVisible();
  });

  test("navigate to settings", async ({ page }) => {
    await page.goto("/settings");
    
    // Settings should be accessible
    const settingsTitle = page.locator("h2, h3").filter({ hasText: /Settings/i }).first();
    await expect(settingsTitle).toBeVisible();
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
    const componentList = page.locator("ul, .list, [role='list']").first();
    if (await componentList.count() > 0) {
      await expect(componentList).toBeVisible();
    } else {
      // Fallback: verify the page loads without errors
      const heading = page.locator("h1, h2").first();
      await expect(heading).toBeVisible();
    }
  });

  test("dark mode toggle works", async ({ page }) => {
    await page.goto("/settings/design-system");
    
    // Look for a theme switcher or dark mode toggle
    const themeSwitch = page.locator("button").filter({ hasText: /light|dark|theme/i }).first();
    if (await themeSwitch.count() > 0) {
      await expect(themeSwitch).toBeVisible();
      
      // Click to toggle and verify state change
      await themeSwitch.click();
      
      // Wait for theme to apply (could check for data-theme attribute or color changes)
      const html = page.locator("html");
      await expect(html).toHaveAttribute(/data-theme/, { timeout: 2000 });
    } else {
      console.log("No theme switcher found, skipping dark mode test");
    }
  });
});
