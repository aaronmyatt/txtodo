// tasks/desktop-workspace-nav-sidebar: WorkspaceSwitcher.svelte's dismissable left-nav sidebar —
// no Playwright coverage existed for the old anchored dropdown (checked before writing this file,
// per that task's own notes.md), so this is new coverage, not a re-point.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("toggle opens the sidebar, focuses it, and Esc closes it and returns focus to the toggle", async ({
	page
}) => {
	const toggle = page.getByRole("button", { name: "Open workspace navigation" });
	await expect(toggle).toBeVisible();

	await toggle.click();
	const sidebar = page.getByRole("menu");
	await expect(sidebar).toBeVisible();
	// Focus moved into the sidebar on open (plan §3.3: "never leave focus stranded").
	await expect(sidebar).toContainText("Manage workspaces");
	// The first control in the menu has the keyboard.
	await expect(sidebar.locator(":focus")).toHaveCount(1);

	await page.keyboard.press("Escape");
	await expect(sidebar).toBeHidden();
	await expect(page.getByRole("button", { name: "Open workspace navigation" })).toBeFocused();
});

test("clicking outside the sidebar dismisses it", async ({ page }) => {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	const sidebar = page.getByRole("menu");
	await expect(sidebar).toBeVisible();

	// Anywhere clearly outside the sidebar's own bounds and the toggle button.
	await page.mouse.click(700, 400);
	await expect(sidebar).toBeHidden();
});

test("the menu lists workspaces and no longer has an Activity tab (it is in Settings)", async ({ page }) => {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	const sidebar = page.getByRole("menu");
	await expect(sidebar).toContainText(daemon.dir);
	await expect(page.getByRole("tab", { name: "Activity" })).toHaveCount(0);
	await expect(sidebar.getByRole("link", { name: "Manage workspaces" })).toBeVisible();
});

// Task workspace-vanity-name: the name every paired device shows, set from a workspace's row.
test("a row's rename sets the shown name; Esc cancels it without closing the sidebar", async ({ page }) => {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	const sidebar = page.getByRole("menu");
	const row = sidebar.getByRole("listitem").filter({ hasText: daemon.dir });
	const input = sidebar.getByRole("textbox", { name: /^Name for / });

	await row.getByRole("button", { name: /^Rename / }).click();
	await expect(input).toBeFocused();
	await page.keyboard.press("Escape");
	await expect(sidebar).toBeVisible();
	await expect(input).toBeHidden();

	await row.getByRole("button", { name: /^Rename / }).click();
	await input.fill("Groceries");
	await input.press("Enter");
	await expect(input).toBeHidden();
	await expect(row).toContainText("Groceries");
});

// nav.workspace_menu and workspace_menu.*: W opens the menu on the open workspace's row; j/k move;
// W closes it again.
test("W opens the menu with the keyboard on a row; j/k move; W closes", async ({ page }) => {
	await expect(page.locator(".buffer .cm-content")).toBeVisible();
	await page.keyboard.press("Shift+W");
	const sidebar = page.getByRole("menu");
	await expect(sidebar).toBeVisible();
	const focused = () => page.evaluate(() => document.activeElement?.classList.contains("entry") ?? false);
	await expect.poll(focused).toBe(true);
	await page.keyboard.press("j");
	await expect.poll(focused).toBe(true);
	await page.keyboard.press("k");
	await expect.poll(focused).toBe(true);
	await page.keyboard.press("Shift+W");
	await expect(sidebar).toBeHidden();
});
