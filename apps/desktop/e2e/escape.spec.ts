// 2026-10-06 feedback: Esc in an editor lets go of the keyboard so the single-key shortcuts work,
// and Esc closes an open detail panel, from an editor inside it as well as from the page.
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

test("Esc in the buffer lets go of the keyboard, so / focuses the search", async ({ page }) => {
	const buffer = page.locator(".buffer .cm-content");
	await buffer.getByText("plan the roadmap").click();
	await expect(buffer).toBeFocused();
	await page.keyboard.press("Escape");
	await expect(buffer).not.toBeFocused();
	await page.keyboard.press("/");
	await expect(page.getByRole("textbox", { name: "Search" })).toBeFocused();
});

test("Esc closes the detail panel from an editor inside it", async ({ page }) => {
	await page.locator(".buffer .cm-content").getByText("plan the roadmap").dblclick();
	const panel = page.getByRole("region", { name: "Task detail" });
	await expect(panel).toBeVisible();
	await panel.locator(".cm-content").first().click();
	await page.keyboard.press("Escape");
	await expect(panel).toHaveCount(0);
});
