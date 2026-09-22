// tasks/desktop-playwright-tests, plan M7 acceptance: "double-click → detail view with pinned
// parent and sub-list." Uses the "nested" fixture, which pre-seeds a ref: tag, a resolvable id:
// (see fixtures.ts's module doc on identity_mode), and an existing sub-directory with its own
// todo.txt containing tasks — so the sub-list renders, and the notes.md beside it, per
// DetailView.svelte (task desktop-notes-hidden).
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

test("double-click opens the detail view with pinned parent and sub-list", async ({ page }) => {
	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await expect(line).toBeVisible();
	// The ref indicator resolves `tasks/q4-roadmap/todo.txt` from the root list's own path (task
	// desktop-ref-indicator-path: a `dirOf(path)` at the call site used to break every indicator
	// under the default layout).
	await expect(page.locator(".cm-todotxt-ref-indicator").first()).toContainText("0/1");
	await line.dblclick();

	await expect(page.locator("section.parent")).toContainText("plan the roadmap");

	await expect(page.locator("section.sublist")).toContainText("of");
	await expect(
		page.locator("section.sublist .cm-line", { hasText: "draft the outline" })
	).toBeVisible();

	// The ref has a notes.md too, so the sub-list and the notes both show (task
	// desktop-notes-hidden); the Notes section opens on its own because the file has text.
	await expect(page.locator("section.notes details")).toHaveAttribute("open", "");
	await expect(
		page.locator("section.notes .cm-line", { hasText: "Q4 goals: ship the outline first." })
	).toBeVisible();
});
