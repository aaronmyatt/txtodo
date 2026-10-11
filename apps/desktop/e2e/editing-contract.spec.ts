// tasks/desktop-ui-revamp, "guard the editing contract": the behaviours the revamp must not
// change, one test each. Those already guarded elsewhere are named here, not repeated:
//   click to edit, id: hidden ............ popover.spec.ts
//   blur commits ......................... inline-edit.spec.ts, save.spec.ts
//   Cmd/Ctrl+S commits, Esc discards ..... inline-edit.spec.ts
//   double-click opens the detail ........ detail.spec.ts
// This file adds Cmd/Ctrl+Enter and the done styling. (Read-only under review went with the
// needs_review flags, ADR 0038.)
// Ref: https://playwright.dev/docs/api/class-keyboard#keyboard-press (ControlOrMeta)
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
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
