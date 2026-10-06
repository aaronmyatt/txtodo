// tasks/desktop-activity-cross-workspace: the cross-workspace activity feed, aggregating op-log
// entries across every registered workspace (not just the one open in the main view). It lives in
// Settings > Activity since task desktop-ui-revamp (it was the workspace menu's Activity tab).
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";
import { tmpPrefix } from "./runId";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

/** Settings > Activity, through the header nav (a client-side navigation, so the daemon bridge
 * set up by `gotoWithDaemon` stays). */
async function openActivity(page: import("@playwright/test").Page) {
	await page.getByRole("link", { name: "Settings" }).click();
	return page.locator("section#activity");
}

/** Registers `dir` through the workspace menu's add field, then closes the menu. */
async function addWorkspaceFromMenu(page: import("@playwright/test").Page, dir: string) {
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	await page.getByPlaceholder("/path/to/workspace").fill(dir);
	await page.getByRole("button", { name: "Add", exact: true }).click();
	await expect(page.getByRole("menu")).toContainText(dir);
	await page.keyboard.press("Escape");
}

test("shows this workspace's own activity by default", async ({ page }) => {
	const panel = await openActivity(page);
	// The "todo" fixture's initial adoption of its own seeded todo.txt is itself real activity.
	await expect(panel.locator("li").first()).toBeVisible();
	await expect(panel).not.toContainText("No recent activity");
});

test("merges in a second registered workspace, tagged with its own root", async ({ page }) => {
	const secondDir = mkdtempSync(join(tmpdir(), tmpPrefix("second-")));
	writeFileSync(join(secondDir, "todo.txt"), "(A) second workspace task\n");
	try {
		await addWorkspaceFromMenu(page, secondDir);
		// Registering doesn't open it; the Activity feed's own fan-out is what triggers adoption.
		const panel = await openActivity(page);
		const secondChip = secondDir.split("/").filter(Boolean).pop() ?? secondDir;
		await expect(panel.getByText(secondChip, { exact: true })).toBeVisible();
	} finally {
		rmSync(secondDir, { recursive: true, force: true });
	}
});

test("a registered workspace whose directory is gone is skipped, not blanking the feed", async ({
	page
}) => {
	const goneDir = mkdtempSync(join(tmpdir(), tmpPrefix("gone-")));
	writeFileSync(join(goneDir, "todo.txt"), "(A) will be deleted\n");

	await addWorkspaceFromMenu(page, goneDir);
	// Gone before the daemon ever opens it — `root_exists` is checked fresh on every list, not
	// cached from add-time, so this reproduces a workspace that's registered but unreachable.
	rmSync(goneDir, { recursive: true, force: true });

	const panel = await openActivity(page);
	await expect(panel.locator("li").first()).toBeVisible();
	await expect(panel.locator(".state-error")).toHaveCount(0);
});
