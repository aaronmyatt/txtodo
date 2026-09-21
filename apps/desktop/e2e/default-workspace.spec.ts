// task default-workspace: a fresh profile has no workspace of its own, so the app opens the
// default one, labels it "Default" in the switcher, offers no way to remove it, and a task typed
// into the main view lands in the default workspace's todo.txt.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("fresh");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("a fresh profile shows the default workspace selected, labelled and not removable", async ({
	page
}) => {
	await expect(page.getByText("No workspace selected")).toHaveCount(0);
	await page.getByRole("button", { name: "Open workspace navigation" }).click();
	const current = page.locator("li.current");
	await expect(current).toContainText("Default");
	await expect(current).toContainText(daemon.dir);
	await expect(page.getByRole("button", { name: `Remove ${daemon.dir}` })).toHaveCount(0);
});

test("a task typed into the main view lands in the default workspace's todo.txt", async ({
	page
}) => {
	const todoPath = join(daemon.dir, "todo.txt");
	expect(readFileSync(todoPath, "utf8")).toBe("");

	await page.locator(".cm-content").first().click();
	await page.keyboard.type("first task in the default workspace");
	await page.locator(".top-nav h1").click();

	await expect
		.poll(() => readFileSync(todoPath, "utf8"))
		.toContain("first task in the default workspace");
});
