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
	await expect(sidebar).toContainText("Workspaces");
	await expect(page.getByRole("tab", { name: "Workspaces" })).toBeFocused();

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

test("the tab bar switches between the Workspaces and Activity panels", async ({ page }) => {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	const sidebar = page.getByRole("menu");

	const workspacesTab = page.getByRole("tab", { name: "Workspaces" });
	const activityTab = page.getByRole("tab", { name: "Activity" });
	await expect(workspacesTab).toHaveAttribute("aria-selected", "true");
	await expect(sidebar).toContainText(daemon.dir);

	await activityTab.click();
	await expect(activityTab).toHaveAttribute("aria-selected", "true");
	await expect(workspacesTab).toHaveAttribute("aria-selected", "false");
	// Real content now (desktop-activity-cross-workspace), not a placeholder — the Workspaces
	// panel's own full-path entries are gone, replaced by the Activity feed's refresh control.
	await expect(sidebar.getByRole("button", { name: "Refresh" })).toBeVisible();
	await expect(sidebar).not.toContainText(daemon.dir);

	await workspacesTab.click();
	await expect(workspacesTab).toHaveAttribute("aria-selected", "true");
	await expect(sidebar).toContainText(daemon.dir);
});
