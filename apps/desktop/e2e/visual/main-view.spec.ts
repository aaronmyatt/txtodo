// tasks/desktop-visual-regression, plan M7 acceptance: light/dark goldens for the main view on
// the shared 10k-line fixture (../tenKFixture.ts) — id: tags hidden, ref: + n/m progress
// decorations visible, the "Add a line…" ghost text on the trailing blank line (plan §3.1).
// Runs under both the "light" and "dark" projects (playwright.config.ts); the default snapshot
// path template already segregates goldens by project name, so this one spec produces both.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "../fixtures";
import { gotoWithDaemon } from "../helpers";
import { TEN_K_DOC_LINES } from "../tenKFixture";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("ten-k");
	await gotoWithDaemon(page, daemon);

	// Wait for the daemon connectivity banner to clear and the full 10k-line document to land
	// (the `data-line-count` testability hook — FileView.svelte) before asserting/screenshotting,
	// so the golden never captures a transient "connecting"/empty-doc frame.
	await expect(page.getByText(/^Daemon:/)).toHaveCount(0);
	await expect(page.locator(`[data-line-count='${TEN_K_DOC_LINES}']`)).toBeVisible();
	// The first line's ref: tag resolves against a real sub-directory (fixtures.ts's "ten-k" seed)
	// — wait for that async listFiles()-driven decoration before the screenshot, not just the doc.
	await expect(page.locator(".cm-todotxt-ref-indicator").first()).toBeVisible();
});

test.afterEach(() => {
	daemon.dispose();
});

test("main view: 10k-line fixture shows id-hidden, ref/progress, and add-a-line placeholder", async ({
	page
}) => {
	// Structural assertions first (fast, and pin down *why* a future pixel diff would fail) —
	// same "assert the DOM, not just the pixels" principle as the functional suite one level up.
	await expect(page.locator(".cm-editor")).not.toContainText("id:");
	await expect(page.locator(".cm-todotxt-ref-indicator").first()).toContainText("1/2");
	await expect(page.locator(".cm-todotxt-strike").first()).toBeVisible();
	await expect(page.locator(".cm-lineNumbers")).toHaveCount(0);

	// The placeholder sits on the document's last line, 10k lines down — CM6 only renders lines
	// (and their decorations) within its own scrolled viewport (`.cm-scroller`, CM6's internal
	// scroll element — `.editor-shell` just bounds its height), so scroll there first, then back
	// to the top before the screenshot (the golden always shows line 1's ref: indicator).
	const scroller = page.locator(".cm-scroller");
	await scroller.evaluate((el) => {
		el.scrollTop = el.scrollHeight;
	});
	await expect(page.locator(".cm-todotxt-add-line-placeholder")).toHaveText("Add a line…");
	await scroller.evaluate((el) => {
		el.scrollTop = 0;
	});

	await expect(page).toHaveScreenshot("main-view.png");
});
