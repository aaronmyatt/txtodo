// tasks/desktop-playwright-tests, plan M7 acceptance: "sub-list line double-click nests the
// breadcrumb to `todo.txt › 2 › q4-roadmap/todo.txt › 3`." Reuses the "nested" fixture (see
// fixtures.ts): a parent with a pre-existing ref: sub-directory and its own todo.txt.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("double-clicking a sub-list line nests the breadcrumb", async ({ page }) => {
	const parentLine = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await parentLine.dblclick();

	const subLine = page
		.locator("section.sublist .cm-line", { hasText: "draft the outline" })
		.first();
	await expect(subLine).toBeVisible();
	await subLine.dblclick();

	const breadcrumb = page.locator("nav.breadcrumb");
	await expect(breadcrumb).toContainText("todo.txt");
	await expect(breadcrumb).toContainText("q4-roadmap/todo.txt");
	// Both levels are addressable crumbs: two "Home"-relative file crumbs plus the current one.
	await expect(breadcrumb.locator("button.crumb")).toHaveCount(3);

	// The nested level's own pinned parent is now the sub-list line, not the top-level task.
	await expect(page.locator("section.parent")).toContainText("draft the outline");
});
