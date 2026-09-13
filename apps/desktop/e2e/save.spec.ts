// tasks/desktop-playwright-tests, plan M7 acceptance (superseded): editing a line inline and
// blurring commits it; the on-disk file changes only that line — byte-diff before/after. Reads
// the file directly from the tempdir, not through the daemon, per notes.md's own guidance.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("editing a line inline and blurring commits it, changing only that line on disk", async ({
	page
}) => {
	const todoPath = join(daemon.dir, "todo.txt");
	const before = readFileSync(todoPath, "utf8");

	const line = page.locator(".cm-line", { hasText: "call mum" }).first();
	await line.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +home");

	// Commit happens on blur (FileView.svelte: "blur or Cmd/Ctrl+S the buffer goes through the
	// reconciler") — click something outside the editor entirely.
	await page.locator(".add-line-row input").click();

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
