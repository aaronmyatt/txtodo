// tasks/desktop-detail-view (superseded edit-popover flow): the pinned parent line in the detail
// view edits inline, the same as any other line in the app (DetailView.svelte's own module doc) —
// click it, type, Enter commits, Escape discards. No popover.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
	await page.locator(".cm-line", { hasText: "plan the roadmap" }).first().dblclick();
});

test.afterEach(() => {
	daemon.dispose();
});

test("editing the pinned parent line and pressing Enter commits it", async ({ page }) => {
	const todoPath = join(daemon.dir, "todo.txt");

	const parentLine = page.locator("section.parent .parent-line .cm-line").first();
	await expect(parentLine).toContainText("plan the roadmap");

	await parentLine.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +urgent");
	await page.keyboard.press("Enter");

	await expect.poll(() => readFileSync(todoPath, "utf8")).toContain("+urgent");
	const lines = readFileSync(todoPath, "utf8").trim().split("\n");
	expect(lines).toHaveLength(1); // an edit, not a whole-file replace: still one line
});

test("Escape discards a pinned-parent edit without writing the file", async ({ page }) => {
	const todoPath = join(daemon.dir, "todo.txt");
	const before = readFileSync(todoPath, "utf8");

	const parentLine = page.locator("section.parent .parent-line .cm-line").first();
	await parentLine.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" scratch, discard me");
	await page.keyboard.press("Escape");

	await expect(parentLine).not.toContainText("discard me");
	await expect(parentLine).toContainText("plan the roadmap");

	// No `Apply` call was ever made — assert the negative after giving the (nonexistent) write a
	// moment it would have needed to land.
	await page.waitForTimeout(300);
	expect(readFileSync(todoPath, "utf8")).toBe(before);
});
