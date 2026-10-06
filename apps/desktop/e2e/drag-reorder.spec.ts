// list.move_up / list.move_down's "or drag the row": drag a line by its number in the gutter and
// drop it above another; the reorder saves through the daemon on its own.
import { readFileSync } from "node:fs";
import { join } from "node:path";
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

test("dragging line 2's number above line 1 moves it to the top", async ({ page }) => {
	const numbers = page.locator(".buffer .cm-lineNumbers .cm-gutterElement");
	const from = await numbers.filter({ hasText: /^2$/ }).boundingBox();
	const first = await page.locator(".buffer .cm-line").first().boundingBox();
	if (!from || !first) throw new Error("no gutter or line");
	await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
	await page.mouse.down();
	await page.mouse.move(first.x + 20, first.y + 2, { steps: 5 });
	await expect(page.locator(".buffer .cm-drop-before")).toHaveCount(1);
	await page.mouse.up();
	await expect.poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8")).toMatch(/^call mum @phone\n\(A\) buy milk/);
});
