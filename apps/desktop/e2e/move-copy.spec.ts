// Task add-the-ability-to-move-or-copy-a-task-t: the line's detail moves or copies it to another
// list. A move is the daemon's cross-file Move (the line leaves this list); a copy adds a new task
// there without the `id:` and `ref:`, and the original stays.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("parity");
	await gotoWithDaemon(page, daemon);
	await expect(page.locator(".buffer .cm-content")).toContainText("call mum");
});

test.afterEach(() => {
	daemon.dispose();
});

const read = (rel: string) => readFileSync(join(daemon.dir, rel), "utf8");
const SUB = "tasks/q4-roadmap/todo.txt";

test("Move to… takes the line to the other list and closes its detail", async ({ page }) => {
	await page.locator(".buffer .cm-line", { hasText: "call mum" }).dblclick();
	await expect(page.locator("section.parent")).toContainText("call mum");
	await page.getByRole("combobox", { name: "Move to another list" }).selectOption(SUB);
	await expect.poll(() => read("todo.txt")).not.toContain("call mum");
	await expect.poll(() => read(SUB)).toContain("call mum");
	await expect(page.locator("section.parent")).toHaveCount(0);
});

test("Copy to… adds a new task there without its ref, and the original stays", async ({ page }) => {
	await page.locator(".buffer .cm-line", { hasText: "plan the roadmap" }).dblclick();
	await expect(page.locator("section.parent")).toContainText("plan the roadmap");
	await page.getByRole("combobox", { name: "Copy to another list" }).selectOption(SUB);
	await expect(page.getByRole("status").filter({ hasText: `Copied to ${SUB}` })).toBeVisible();
	await expect.poll(() => read(SUB)).toMatch(/plan the roadmap\n?$/);
	expect(read(SUB)).not.toContain("ref:");
	expect(read("todo.txt")).toContain("plan the roadmap ref:q4-roadmap");
});

// list.move_to / list.copy_to: in list mode, m opens the detail with its Move to… picker focused.
test("m opens the line's detail with the Move to… picker focused", async ({ page }) => {
	await page.keyboard.press("j");
	await page.keyboard.press("m");
	await expect(page.locator("section.parent")).toContainText("call mum");
	await expect(page.getByRole("combobox", { name: "Move to another list" })).toBeFocused();
});
