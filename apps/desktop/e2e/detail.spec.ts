// tasks/desktop-playwright-tests, plan M7 acceptance: "double-click → detail view with pinned
// parent, notes editor, sub-list." Uses the "nested" fixture, which pre-seeds a ref: tag, a
// resolvable id: (see fixtures.ts's module doc on identity_mode), and an existing sub-directory
// with its own todo.txt, so all three sections render their real (not empty-state) content.
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

test("double-click opens the detail view with pinned parent, notes editor, and sub-list", async ({
	page
}) => {
	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await expect(line).toBeVisible();
	await line.dblclick();

	await expect(page.locator("section.parent")).toContainText("plan the roadmap");

	// A resolvable id: is seeded, so the real editor mounts, not DetailView's "no resolvable id"
	// empty state (see DetailView.svelte's notes-section guard).
	await expect(page.locator("section.notes .notes-editor")).toBeVisible();

	await expect(page.locator("section.sublist")).toContainText("of");
	await expect(
		page.locator("section.sublist .cm-line", { hasText: "draft the outline" })
	).toBeVisible();
});
