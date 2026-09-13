// tasks/desktop-visual-regression, plan M7 acceptance / plan §3.2: light/dark goldens for the
// edit popover showing all three things at once — the raw line (hidden id: tag included, unlike
// the main view's decoration), the chips row, and the inline strict-mode validation error.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "../fixtures";
import { gotoWithDaemon, openPopoverFor } from "../helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("edit popover: raw line with id:, chips row, inline validation error", async ({ page }) => {
	const line = page.locator(".cm-line", { hasText: "call mum" }).first();
	await openPopoverFor(page, line);

	const popoverContent = page.locator(".popover .cm-content");
	await expect(popoverContent).toContainText("id:01ARZ3NDEKTSV4RRFFQ69G5FAV");

	// Introduce one double space (a real strict-mode violation — crates/txtodo-core/src/parse.rs's
	// "SP" rule: "words are separated by exactly one space") while keeping the id: tag intact, so
	// the golden shows the raw line + id: tag + chips row + validation error all at once, matching
	// this task's brief. Triple-click selects the whole (single-line) popover content.
	await popoverContent.click({ clickCount: 3 });
	await page.keyboard.type("(A) call  mum id:01ARZ3NDEKTSV4RRFFQ69G5FAV");

	const error = page.locator(".popover .error");
	await expect(error).toBeVisible();
	await expect(error).toContainText("exactly one space");

	await expect(page.locator(".popover .chips button")).toHaveCount(9);

	// CM6's cursor-blink animation is a CSS `animation` (`cm-blink`) — `toHaveScreenshot`'s default
	// `animations: "disabled"` freezes it at its initial frame, so no manual handling is needed
	// (see ../visual/deterministic.ts's module doc for the full explanation).
	await expect(page.locator(".popover")).toHaveScreenshot("edit-popover.png");
});
