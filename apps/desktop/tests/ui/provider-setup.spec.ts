import { expect, test } from "@playwright/test";

test("9Router setup tests and imports model IDs into the selected connection", async ({ page }) => {
  await page.goto("/settings");
  await expect(page.getByLabel("Legacy router URL")).toHaveAccessibleDescription(/legacy default model/);
  await page.getByRole("link", { name: "Set up 9Router" }).click();

  const provider = page.getByRole("combobox", { name: "Provider", exact: true });
  const name = page.locator("form").filter({ has: provider }).getByLabel("Name", { exact: true });
  const models = page.getByLabel("Models", { exact: true });
  const baseUrl = page.getByLabel("Base URL", { exact: true });
  const key = page.getByLabel("API key", { exact: true });
  const importModels = page.getByRole("button", { name: "Import tested models" });
  await expect(provider).toHaveValue("9router");
  await expect(baseUrl).toHaveAttribute("placeholder", "http://127.0.0.1:20128/v1");
  await expect(key).toHaveAccessibleDescription("Enter a 9Router API key if your server requires one.");
  await name.fill("Local 9Router");
  await models.fill("manual/model\nocg/coder");
  await key.fill("mock-key");
  await page.getByRole("button", { name: "Add connection", exact: true }).click();

  await provider.selectOption("9router");
  await name.fill("Second 9Router");
  await baseUrl.fill("http://localhost:30128/v1");
  await page.getByRole("button", { name: "Add connection", exact: true }).click();

  await page.getByRole("button", { name: "Test Local 9Router", exact: true }).click();
  await expect(page.getByText(/2 models available/)).toBeVisible();
  await page.getByRole("button", { name: "Edit Second 9Router", exact: true }).click();
  await expect(importModels).toBeHidden();
  await page.getByRole("button", { name: "Edit Local 9Router", exact: true }).click();
  await expect(key).toBeEmpty();
  await expect(importModels).toBeVisible();

  await baseUrl.fill("http://localhost:40128/v1");
  await expect(importModels).toBeHidden();
  await baseUrl.clear();
  await importModels.click();
  await importModels.click();
  await expect(models).toHaveValue("manual/model\nocg/coder\nalias,variant");
  const configuredSuggestion = page.locator('#targets option[value^="connection:"][value$="/ocg/coder"]');
  await expect(configuredSuggestion).toHaveCount(1);
  const configuredValue = await configuredSuggestion.getAttribute("value");
  const importedSuggestion = page.locator(`#targets option[value="${configuredValue?.replace(/\/ocg\/coder$/, "/alias,variant")}"]`);
  await expect(importedSuggestion).toHaveCount(0);
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(importedSuggestion).toHaveCount(1);
  await expect(configuredSuggestion).toHaveCount(1);
});
