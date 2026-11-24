import { expect, test } from "@playwright/test";

test("command palette filters and opens routes with the keyboard", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("navigation", { name: "Activity modes" }).getByRole("link", { name: "Chat" }).focus();
  await page.keyboard.press("Control+k");

  const palette = page.getByRole("dialog", { name: "Command palette" });
  const search = palette.getByRole("combobox", { name: "Search commands" });
  await expect(search).toBeFocused();
  await search.fill("not a command");
  await expect(palette.getByRole("status")).toHaveText("No matching commands");

  await search.fill("workspace");
  await expect(palette.getByRole("option")).toHaveCount(2);
  await search.press("ArrowDown");
  await expect(palette.getByRole("option", { name: /Design/ })).toHaveAttribute("aria-selected", "true");
  await search.press("Enter");
  await expect(page).toHaveURL(/\/design$/);
  await expect(palette).toHaveCount(0);
});

test("Escape restores focus and Notes keeps its search shortcut", async ({ page }) => {
  await page.goto("/");
  const chat = page.getByRole("navigation", { name: "Activity modes" }).getByRole("link", { name: "Chat" });
  await chat.focus();
  await page.keyboard.press("Meta+k");

  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
  await expect(chat).toBeFocused();

  await page.goto("/notes");
  await page.getByRole("textbox", { name: "Vault folder path" }).fill("/notes");
  await page.getByRole("button", { name: "Use this folder" }).click();
  await expect(page.getByRole("button", { name: /Search notes/ })).toBeVisible();
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog", { name: "Search notes" })).toBeVisible();
  await expect(palette).toHaveCount(0);
});
