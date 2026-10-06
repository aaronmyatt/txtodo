// specs/client-parity.toml sync.open: the status footer says how sync stands (from the daemon's
// SyncStatus), and s in list mode, or a click, opens the detail; Esc closes it.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
	await expect(page.locator(".buffer .cm-content")).toContainText("call mum");
});

test.afterEach(() => {
	daemon.dispose();
});

test("the footer reads the daemon's sync state, and s opens the detail", async ({ page }) => {
	const indicator = page.getByRole("button", { name: /^Sync: / });
	await expect(indicator).toContainText("no peers · 0 pending");
	await page.keyboard.press("s");
	const popup = page.getByRole("dialog", { name: "Sync details" });
	await expect(popup).toContainText("no peers");
	await page.keyboard.press("Escape");
	await expect(popup).toHaveCount(0);
	await indicator.click();
	await expect(popup).toBeVisible();
});
