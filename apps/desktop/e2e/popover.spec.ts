// tasks/desktop-playwright-tests, plan M7 acceptance (superseded): a line edits directly, like a
// plain text file — click it to drop a cursor, type, and the hidden id: tag stays hidden
// throughout (no separate popover reveals it — FileView.svelte's own module doc).
import { expect, test } from "@playwright/test";
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

test("clicking a line drops a cursor and edits it in place, id: staying hidden throughout", async ({
	page
}) => {
	const line = page.locator(".cm-line", { hasText: "call mum" }).first();
	await expect(line).toBeVisible();
	// Main view hides id: tags by default (§3.1) — confirm it is not in the visible line text.
	await expect(line).not.toContainText("id:");

	await line.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +home");

	await expect(line).toContainText("call mum +home");
	// The decoration hides the tag in the document CM6 renders, not just in a separate read-only
	// projection — it stays hidden while the same line is being typed into.
	await expect(line).not.toContainText("id:");
});
