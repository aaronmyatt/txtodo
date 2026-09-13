// tasks/desktop-visual-regression, plan M7 acceptance: light/dark goldens for the conflict review
// sheet with M4's three resolutions (mine / theirs / merged) all reachable in one render — the
// same reading ../conflict.spec.ts's own assertions confirm (and tasks/desktop-conflict-review's
// notes: "a review sheet offering the three resolutions mine/theirs/merged", one sheet, one flag,
// three buttons — not three separate fixture states). Reuses the "conflict" fixture and the same
// debug-raise substitute for a real second-daemon injection (see ../conflict.spec.ts's module doc
// for why: daemon-to-daemon sync has no transport wired up yet, tracked as sync-loopback-converge).
import { expect, test } from "@playwright/test";
import {
	CONFLICT_MINE,
	CONFLICT_TASK_ID,
	CONFLICT_THEIRS,
	type DaemonHandle,
	debugRaiseConflict,
	spawnDaemon
} from "../fixtures";
import { gotoWithDaemon } from "../helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("conflict");
	await debugRaiseConflict(daemon, {
		path: "todo.txt",
		taskId: CONFLICT_TASK_ID,
		mine: CONFLICT_MINE,
		theirs: CONFLICT_THEIRS
	});
	await gotoWithDaemon(page, daemon);

	const banner = page.getByRole("status").filter({ hasText: "need review" });
	await expect(banner).toBeVisible();
	await banner.getByRole("button", { name: "Review" }).click();
});

test.afterEach(() => {
	daemon.dispose();
});

test("conflict review sheet: diff view and all three resolutions reachable", async ({ page }) => {
	const sheet = page.getByRole("dialog", { name: /review conflicting edit/i });
	await expect(sheet).toBeVisible();
	await expect(sheet.locator(".merged-preview")).toContainText("buy oat milk");
	await expect(sheet.getByRole("button", { name: "keep mine" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep theirs" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep merged" })).toBeVisible();

	await expect(page).toHaveScreenshot("conflict-sheet.png");
});
