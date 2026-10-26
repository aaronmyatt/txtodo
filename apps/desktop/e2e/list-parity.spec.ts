// tasks/desktop-list-parity: the root list and a detail's sub-list are one FileView and must act
// the same. Every check here runs once against each, so the two cannot drift apart again.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
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

interface Target {
	name: string;
	/** The list's file, relative to the workspace. */
	file: string;
	/** Its first and second lines as seeded. */
	first: string;
	second: string;
	/** Shows the list with list mode's keys on it, and returns its container. */
	open: (page: Page) => Promise<Locator>;
}

const TARGETS: Target[] = [
	{
		name: "root list",
		file: "todo.txt",
		first: "plan the roadmap",
		second: "call mum",
		open: async (page) => page.locator(".buffer")
	},
	{
		name: "sub-list",
		file: "tasks/q4-roadmap/todo.txt",
		first: "draft the outline",
		second: "book the venue",
		open: async (page) => {
			// Enter opens the current line's detail; its sub-list takes the keys.
			await page.keyboard.press("Enter");
			const panel = page.getByRole("region", { name: "Task detail" });
			await expect(panel).toContainText("book the venue");
			// The panel holds other editors too (the parent line, the notes): the sub-list is its FileView.
			return panel.locator(".file-view");
		}
	}
];

for (const t of TARGETS) {
	test.describe(t.name, () => {
		const file = () => readFileSync(join(daemon.dir, t.file), "utf8");

		test("j/k move the current line without taking the keyboard", async ({ page }) => {
			const list = await t.open(page);
			await page.keyboard.press("j");
			await expect(list.locator(".cm-activeLine")).toContainText(t.second);
			await page.keyboard.press("k");
			await expect(list.locator(".cm-activeLine")).toContainText(t.first);
			await expect(list.locator(".cm-content")).not.toBeFocused();
		});

		test("J moves the line and u takes it back", async ({ page }) => {
			const list = await t.open(page);
			const before = file();
			await page.keyboard.press("Shift+J");
			await expect.poll(file).toMatch(new RegExp(`^${t.second}`));
			await expect(list.locator(".cm-activeLine")).toContainText(t.first);
			await page.keyboard.press("u");
			await expect.poll(file).toBe(before);
		});

		test("x completes with an Undo toast; d d deletes", async ({ page }) => {
			await t.open(page);
			const before = file();
			await page.keyboard.press("x");
			await expect.poll(file).toMatch(new RegExp(`^x \\d{4}-\\d{2}-\\d{2} .*${t.first}`, "m"));
			await page.getByRole("button", { name: "Undo" }).click();
			await expect.poll(file).toBe(before);
			await page.keyboard.press("j");
			await page.keyboard.press("d");
			await page.keyboard.press("d");
			await expect.poll(file).not.toContain(t.second);
		});

		test("the header search marks hits", async ({ page }) => {
			const list = await t.open(page);
			const word = t.second.split(" ").at(-1) ?? "";
			await page.getByRole("textbox", { name: "Search" }).fill(word);
			await expect(list.locator(".cm-hit")).toHaveText(word);
			await expect(list.locator(".cm-line.cm-dim")).toContainText(t.first);
		});
	});
}

// The add-input decision (2026-10-11, tasks/desktop-list-parity/notes.md): no list has its own add
// input; the prompt bar (n) adds at the top of whichever list has the keys.
for (const t of TARGETS) {
	test(`${t.name}: n and the prompt bar add at the top of this list`, async ({ page }) => {
		await t.open(page);
		const file = () => readFileSync(join(daemon.dir, t.file), "utf8");
		await page.keyboard.press("n");
		await expect(page.getByRole("textbox", { name: "Add a task" })).toBeFocused();
		await page.keyboard.type("water the plants");
		await page.keyboard.press("Enter");
		await expect.poll(file).toMatch(/^\d{4}-\d{2}-\d{2} water the plants/);
		await expect(page.getByRole("textbox", { name: "Add a line" })).toHaveCount(0);
	});
}
