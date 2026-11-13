import { expect, test } from "@playwright/test";

/**
 * Interaction checks for the optical-atelier shell against the dev mock.
 * `main.tsx` installs the Tauri bridge shim only in dev builds, so these run
 * against `vite`, never a release bundle.
 */
test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=1");
  await expect(page.getByRole("navigation", { name: "Workspace sections" })).toBeVisible();
});

test("the shell exposes its landmarks and the skip link", async ({ page }) => {
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("The disc");
  await expect(page.getByRole("main")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Session" })).toBeVisible();
  await expect(page.locator(".statusbar").getByText("keepsake atelier")).toBeVisible();
  await expect(page.getByRole("button", { name: "Keyboard shortcuts" })).toBeVisible();

  const skip = page.getByRole("link", { name: "Skip to the workspace" });
  await expect(skip).not.toBeInViewport();
  await page.keyboard.press("Tab");
  await expect(skip).toBeFocused();
  await expect(skip).toBeInViewport();
  await skip.press("Enter");
  await expect(page.getByRole("main")).toBeFocused();
});

test("reads the mock disc and lists its titles", async ({ page }) => {
  await page.getByRole("button", { name: /Pioneer/ }).click();
  await page.getByRole("button", { name: "Read disc" }).click();

  await expect(page.getByRole("heading", { name: "Titles" })).toBeVisible();
  await expect(page.locator(".title")).toHaveCount(6);
  await expect(page.getByText("AACS, decryptable")).toBeVisible();

  await page.locator(".title").nth(1).click();
  await expect(page.locator(".title").nth(1)).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".title").first()).toHaveAttribute("aria-pressed", "false");
});

test("the command palette filters, navigates and closes on escape", async ({ page }) => {
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();

  const search = page.getByRole("textbox", { name: "Search commands" });
  await expect(search).toBeFocused();
  await search.fill("key");
  await expect(page.getByRole("option", { name: /Update decryption keys/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();

  await page.getByRole("button", { name: "Open the command palette" }).click();
  await expect(palette).toBeVisible();
  await search.fill("rip settings");
  await page.getByRole("option", { name: /Rip settings/ }).click();
  await expect(palette).toBeHidden();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Rip settings");
});

test("the shortcuts sheet opens with ? and closes with escape", async ({ page }) => {
  await page.keyboard.press("?");
  const sheet = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByText("Open the command palette")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
});

test("collapsing the rail keeps the sections reachable", async ({ page }) => {
  await page.getByRole("button", { name: "Collapse navigation" }).click();
  await expect(page.getByRole("button", { name: "Expand navigation" })).toBeVisible();
  await page.getByRole("button", { name: /^Rip settings\./ }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Rip settings");
});

test("reduced motion removes running animations", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.reload();
  await expect(page.getByRole("navigation", { name: "Workspace sections" })).toBeVisible();
  // Reduced motion collapses durations to ~0, so mount transitions may still be
  // in `running` for a tick. Nothing may keep animating afterwards.
  await expect
    .poll(async () =>
      page.evaluate(() => document.getAnimations().filter((a) => a.playState === "running").length),
    )
    .toBe(0);
});

test("System shows the running version and the changelog", async ({ page }) => {
  await page.getByRole("button", { name: /^System\./ }).click();
  await expect(page.getByRole("heading", { name: "Version" })).toBeVisible();
  await expect(page.locator(".about-version")).toHaveText("0.2.0");
  await page.getByText("Changelog", { exact: true }).click();
  await expect(page.getByText("Nothing yet.")).toBeVisible();
});

test("captures the populated disc view", async ({ page }) => {
  await page.getByRole("button", { name: /Pioneer/ }).click();
  await page.getByRole("button", { name: "Read disc" }).click();
  await expect(page.getByRole("heading", { name: "Titles" })).toBeVisible();
  await page.screenshot({ path: "test-results/ui-disc.png", fullPage: true });
});
