// tasks/desktop-ui-revamp, decision 2 (2026-09-27): the detail opens as a bottom panel. The
// buffer above stays visible and editable; the close button and Esc (outside an editor) close it.
import { readFileSync } from "node:fs";
import { join } from "node:path";
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

test("the detail opens under the buffer and closes by button or Esc", async ({ page }) => {
	const buffer = page.locator(".buffer .cm-content");
	const panel = page.getByRole("region", { name: "Task detail" });
	await buffer.getByText("plan the roadmap").dblclick();
	await expect(panel).toBeVisible();
	await expect(panel).toContainText("draft the outline");
	await expect(buffer).toBeVisible();

	await page.getByRole("button", { name: "Close detail" }).click();
	await expect(panel).toHaveCount(0);

	await buffer.getByText("plan the roadmap").dblclick();
	await expect(panel).toBeVisible();
	// Esc belongs to an editor while one has the keyboard; from the page it closes the panel.
	await page.locator(".subbar").click();
	await page.keyboard.press("Escape");
	await expect(panel).toHaveCount(0);
});

// tasks/desktop-list-parity: list mode's keys act on the active list. An open detail's sub-list
// takes them; once the panel closes they are the root list's again.
test("list keys act on an open detail's sub-list, then on the root again", async ({ page }) => {
	const read = (rel: string) => readFileSync(join(daemon.dir, rel), "utf8");
	const panel = page.getByRole("region", { name: "Task detail" });
	await expect(page.locator(".buffer .cm-content")).toContainText("plan the roadmap");
	await page.keyboard.press("Enter");
	await expect(panel).toContainText("draft the outline");
	await page.keyboard.press("x");
	await expect.poll(() => read("tasks/q4-roadmap/todo.txt")).toMatch(/^x \d{4}-\d{2}-\d{2} .*draft the outline/);
	expect(read("todo.txt")).toMatch(/^\(A\) plan the roadmap/);

	await page.keyboard.press("Escape");
	await expect(panel).toHaveCount(0);
	await page.keyboard.press("x");
	await expect.poll(() => read("todo.txt")).toMatch(/^x \d{4}-\d{2}-\d{2} .*plan the roadmap/);
});
