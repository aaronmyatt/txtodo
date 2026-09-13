// tasks/desktop-playwright-tests, plan M7 acceptance: "click a line → popover pre-filled with the
// raw line including the hidden id: tag." DOM-structural only — see playwright.config.ts's header.
import { expect, test } from "@playwright/test";
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

test("click a line opens the popover pre-filled with the raw line, hidden id: tag included", async ({
	page
}) => {
	const line = page.locator(".cm-line", { hasText: "call mum" }).first();
	await expect(line).toBeVisible();
	// Main view hides id: tags by default (§3.1) — confirm it is not in the visible line text.
	await expect(line).not.toContainText("id:");

	await openPopoverFor(page, line);

	// ...but the popover shows the RAW line, id: included (notes.md: "unlike the main view's
	// decoration, this popover shows everything").
	const popoverLine = page.locator(".popover .cm-content");
	await expect(popoverLine).toContainText("call mum");
	await expect(popoverLine).toContainText("id:01ARZ3NDEKTSV4RRFFQ69G5FAV");
});
