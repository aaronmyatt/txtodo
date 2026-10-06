// specs/client-parity.toml list.*: with nothing holding the keyboard, single keys act on the
// Tasks buffer's current line (the caret's), through the daemon, as in the TUI.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
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

const file = () => readFileSync(join(daemon.dir, "todo.txt"), "utf8");
const current = (page: Page) => page.locator(".buffer .cm-activeLine");

test("j/k/G/g g move the current line without taking the keyboard", async ({ page }) => {
	await page.keyboard.press("j");
	await expect(current(page)).toContainText("call mum");
	await page.keyboard.press("k");
	await expect(current(page)).toContainText("buy milk");
	await page.keyboard.press("Shift+G");
	await expect(current(page)).toContainText("call mum");
	await page.keyboard.press("g");
	await page.keyboard.press("g");
	await expect(current(page)).toContainText("buy milk");
	await expect(page.locator(".buffer .cm-content")).not.toBeFocused();
});

test("x completes the line, J moves it, d d deletes it", async ({ page }) => {
	await page.keyboard.press("Shift+J");
	await expect.poll(file).toMatch(/^call mum @phone\n\(A\) buy milk/);
	await expect(current(page)).toContainText("buy milk");

	await page.keyboard.press("x");
	await expect.poll(file).toMatch(/^x \d{4}-\d{2}-\d{2} .*buy milk/m);

	await page.keyboard.press("g");
	await page.keyboard.press("g");
	await page.keyboard.press("d");
	await page.keyboard.press("d");
	await expect.poll(file).not.toContain("call mum");
});

test("i edits the line, and Enter opens its detail", async ({ page }) => {
	await page.keyboard.press("j");
	await page.keyboard.press("i");
	await expect(page.locator(".buffer .cm-content")).toBeFocused();
	await page.keyboard.press("Escape");
	await page.keyboard.press("Enter");
	await expect(page.getByRole("region", { name: "Task detail" })).toContainText("call mum");
});
