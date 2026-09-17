// tasks/desktop-activity-cross-workspace: the nav sidebar's Activity tab, aggregating op-log
// entries across every registered workspace (not just the one open in the main view).
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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

async function openActivityTab(page: import("@playwright/test").Page) {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	await page.getByRole("tab", { name: "Activity" }).click();
}

test("shows this workspace's own activity by default", async ({ page }) => {
	await openActivityTab(page);
	const panel = page.locator("#nav-panel-activity");
	// The "todo" fixture's initial adoption of its own seeded todo.txt is itself real activity.
	await expect(panel.locator("li").first()).toBeVisible();
	await expect(panel).not.toContainText("No recent activity");
});

test("merges in a second registered workspace, tagged with its own root", async ({ page }) => {
	const secondDir = mkdtempSync(join(tmpdir(), "txtodo-e2e-second-"));
	writeFileSync(join(secondDir, "todo.txt"), "(A) second workspace task\n");
	try {
		await page.getByRole("button", { name: "Open workspace navigation" }).click();
		await page.getByPlaceholder("/path/to/workspace").fill(secondDir);
		await page.getByRole("button", { name: "Add" }).click();
		// Registering doesn't open it; the Activity tab's own fan-out is what triggers adoption.
		await page.getByRole("tab", { name: "Activity" }).click();

		const panel = page.locator("#nav-panel-activity");
		const secondChip = secondDir.split("/").filter(Boolean).pop() ?? secondDir;
		await expect(panel.getByText(secondChip, { exact: true })).toBeVisible();
	} finally {
		rmSync(secondDir, { recursive: true, force: true });
	}
});

test("a registered workspace whose directory is gone is skipped, not blanking the feed", async ({
	page
}) => {
	const goneDir = mkdtempSync(join(tmpdir(), "txtodo-e2e-gone-"));
	writeFileSync(join(goneDir, "todo.txt"), "(A) will be deleted\n");

	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	await page.getByPlaceholder("/path/to/workspace").fill(goneDir);
	await page.getByRole("button", { name: "Add" }).click();
	// Gone before the daemon ever opens it — `root_exists` is checked fresh on every list, not
	// cached from add-time, so this reproduces a workspace that's registered but unreachable.
	rmSync(goneDir, { recursive: true, force: true });

	await page.getByRole("tab", { name: "Activity" }).click();
	const panel = page.locator("#nav-panel-activity");
	await expect(panel.locator("li").first()).toBeVisible();
	await expect(panel.locator(".state-error")).toHaveCount(0);
});
