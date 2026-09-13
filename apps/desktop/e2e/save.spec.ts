// tasks/desktop-playwright-tests, plan M7 acceptance: "Enter saves; on-disk file changes only
// that line — byte-diff before/after." Reads the file directly from the tempdir, not through the
// daemon, per notes.md's own guidance.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon, openPopoverFor } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("Enter saves and the on-disk file changes only that line", async ({ page }) => {
	const todoPath = join(daemon.dir, "todo.txt");
	const before = readFileSync(todoPath, "utf8");

	const line = page.locator(".cm-line", { hasText: "call mum" }).first();
	await openPopoverFor(page, line);

	const editor = page.locator(".popover .cm-content");
	await editor.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +home");
	await page.keyboard.press("Enter");

	// The popover closes only after `onSave` resolves (EditPopover.svelte's `saveAndClose`).
	await expect(page.locator(".popover")).toHaveCount(0);

	await expect.poll(() => readFileSync(todoPath, "utf8")).not.toBe(before);

	const after = readFileSync(todoPath, "utf8");
	const beforeLines = before.split("\n");
	const afterLines = after.split("\n");
	expect(afterLines.length).toBe(beforeLines.length);

	let changedLines = 0;
	for (let i = 0; i < beforeLines.length; i++) {
		if (beforeLines[i] !== afterLines[i]) {
			changedLines++;
			expect(afterLines[i]).toContain("+home");
		}
	}
	expect(changedLines).toBe(1);
});
