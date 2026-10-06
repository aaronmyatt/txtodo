// tasks/desktop-ui-revamp, "guard the editing contract": the behaviours the revamp must not
// change, one test each. Those already guarded elsewhere are named here, not repeated:
//   click to edit, id: hidden ............ popover.spec.ts
//   blur commits ......................... inline-edit.spec.ts, save.spec.ts
//   Cmd/Ctrl+S commits, Esc discards ..... inline-edit.spec.ts
//   double-click opens the detail ........ detail.spec.ts
// This file adds Cmd/Ctrl+Enter, the done styling and read-only under review.
// Ref: https://playwright.dev/docs/api/class-keyboard#keyboard-press (ControlOrMeta)
import { expect, test } from "@playwright/test";
import {
	CONFLICT_MINE,
	CONFLICT_TASK_ID,
	CONFLICT_THEIRS,
	type DaemonHandle,
	debugRaiseConflict,
	spawnDaemon
} from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle | undefined;

test.afterEach(() => {
	daemon?.dispose();
	daemon = undefined;
});

test("Cmd/Ctrl+Enter on a line opens its detail", async ({ page }) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await line.click();
	await page.keyboard.press("ControlOrMeta+Enter");
	await expect(page.locator("section.parent")).toContainText("plan the roadmap");
});

test("a done line is dimmed and its words struck, not its marker or dates", async ({ page }) => {
	daemon = await spawnDaemon("done");
	await gotoWithDaemon(page, daemon);
	const done = page.locator(".cm-line.cm-todotxt-done", { hasText: "pay rent" });
	await expect(done).toBeVisible();
	const strike = done.locator(".cm-todotxt-strike");
	await expect(strike.first()).toContainText("pay rent");
	await expect(strike.first()).not.toContainText("2026-10-01");
	await expect(page.locator(".cm-line", { hasText: "call mum" })).not.toHaveClass(/cm-todotxt-done/);
});

test("the buffer is read-only while a line needs review", async ({ page }) => {
	daemon = await spawnDaemon("conflict");
	await debugRaiseConflict(daemon, {
		path: "todo.txt",
		taskId: CONFLICT_TASK_ID,
		mine: CONFLICT_MINE,
		theirs: CONFLICT_THEIRS
	});
	await gotoWithDaemon(page, daemon);
	await expect(page.getByRole("button", { name: "Review" })).toBeVisible();
	const content = page.locator(".buffer .cm-content");
	await expect(content).toHaveAttribute("contenteditable", "false");
	await page.locator(".cm-line").first().click();
	await page.keyboard.type("zzz");
	await expect(page.locator(".cm-line").first()).not.toContainText("zzz");
});
