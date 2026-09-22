// tasks/desktop-sublist-start: a task with no `ref:` yet gets a sub-task from the detail view.
// The first submit claims the ref: directory (`RefDir { ensure }`) and adds the line to the new
// `todo.txt` through `apply`; opening the view and typing alone writes nothing (the same negative
// notes-create.spec.ts asserts for notes).
import { expect, test } from "@playwright/test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("notes-create");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

/** The ref dirs the daemon has made under the default `tasks/` layout (task workspace-layout). */
function refDirs(): string[] {
	const tasks = join(daemon.dir, "tasks");
	return existsSync(tasks) ? readdirSync(tasks) : [];
}

test("the first sub-task of a task with no ref: creates its directory and todo.txt", async ({ page }) => {
	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await line.dblclick();

	const input = page.locator('section.sublist input[aria-label="Add a sub-task"]');
	await expect(input).toBeVisible();
	await expect(input).toBeEnabled();
	// Negative: opening the view and typing writes nothing until Enter.
	await input.fill("draft the outline");
	expect(refDirs()).toEqual([]);

	await input.press("Enter");
	await expect.poll(() => refDirs().length, { timeout: 5000 }).toBe(1);
	const slug = refDirs()[0];
	const subList = join(daemon.dir, "tasks", slug, "todo.txt");
	await expect.poll(() => existsSync(subList)).toBe(true);
	expect(readFileSync(subList, "utf8")).toContain("draft the outline");
	expect(readFileSync(join(daemon.dir, "todo.txt"), "utf8")).toContain(`ref:${slug}`);

	// The input hands over to the real sub-list view once the daemon lists the new file.
	await expect(page.locator("section.sublist .cm-line", { hasText: "draft the outline" })).toBeVisible();
	await expect(page.locator("section.sublist")).toContainText("0 of 1 done");
});
