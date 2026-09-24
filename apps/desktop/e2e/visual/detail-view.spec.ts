// tasks/desktop-visual-regression, plan M7 acceptance / plan §3.2: light/dark goldens for the
// detail view — pinned parent, sub-list, breadcrumb, footer with the directory path. Reuses
// the "nested" fixture (../fixtures.ts) exactly as ../detail.spec.ts does, so both suites describe
// the same real, resolvable-id, has-a-sub-list document (sub-list wins over notes here — mutual
// exclusivity, see DetailView.svelte).
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "../fixtures";
import { gotoWithDaemon } from "../helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);

	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await line.dblclick();
	await expect(page.locator("section.parent")).toContainText("plan the roadmap");
	await expect(page.locator("section.notes")).toBeVisible();
	await expect(page.locator("section.sublist")).toContainText("of");
});

test.afterEach(() => {
	daemon.dispose();
});

test("detail view: pinned parent, sub-list, breadcrumb, footer directory path", async ({
	page
}) => {
	await expect(page.locator("nav.breadcrumb")).toContainText("Home");
	await expect(page.locator("nav.breadcrumb")).toContainText("todo.txt");
	await expect(page.locator(".detail-footer .dir")).not.toBeEmpty();

	// The footer's directory path is the real, per-test `mkdtempSync` tempdir
	// (e2e/fixtures.ts::spawnDaemon) — a different absolute path (and thus a different text
	// length/wrap) on every run by construction, unlike anything else on this screen. Masking it
	// (https://playwright.dev/docs/api/class-pageassertions#page-assertions-to-have-screenshot-1)
	// is the standard Playwright answer for "one region is legitimately non-deterministic": it
	// still asserts the region *exists* (the .not.toBeEmpty() above) and stays in place, just not
	// its exact pixels.
	// Mask the WHOLE footer block element, not just the `.dir` span inside it: `.dir`'s rendered
	// width shifts by a few px run-to-run (same character *count* every time — mkdtempSync always
	// appends exactly 6 random chars — but a proportional font, `.detail-view`'s own
	// Inter/Avenir/Helvetica/Arial stack, gives different random characters different glyph
	// widths), which shifted the mask rectangle's own right edge and leaked a few boundary pixels
	// into the diff (confirmed by inspecting test-results/.../detail-view-diff.png). `.detail-footer`
	// is a block element that always spans the full content width regardless of its text, so
	// masking it instead gives a fixed-size rectangle with nothing left to leak.
	await expect(page).toHaveScreenshot("detail-view.png", {
		mask: [page.locator(".detail-footer")]
	});
});
