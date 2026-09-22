// tasks/layout-client-gaps: a workspace whose root list is not `todo.txt`. The main view must
// open `work.txt` (the daemon's `workspace_layout` answer), an edit must land in `work.txt`, and
// nothing may create a `todo.txt` on the side — the silent wrong answer every missed code path
// used to give.
import { expect, test } from "@playwright/test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("custom-root");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("the main view opens work.txt and an edit lands there, never in a todo.txt", async ({ page }) => {
	const workPath = join(daemon.dir, "work.txt");
	const line = page.locator(".cm-line", { hasText: "plan the launch" }).first();
	await expect(line).toBeVisible();

	await line.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +launch");
	await page.keyboard.press("ControlOrMeta+s");

	await expect.poll(() => readFileSync(workPath, "utf8")).toContain("+launch");
	expect(existsSync(join(daemon.dir, "todo.txt"))).toBe(false);

	// The detail view reads the same document: its pinned parent is the work.txt line.
	await line.dblclick();
	await expect(page.locator("section.parent")).toContainText("plan the launch");
	expect(existsSync(join(daemon.dir, "todo.txt"))).toBe(false);
});
