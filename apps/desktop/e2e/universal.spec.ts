// tasks/desktop-ui-revamp: the Universal page against a real daemon (the bridge runs the shipped
// `universal_rows` / `set_done` / `undo`). Rows come from every registered workspace; the round
// mark completes in the row's own workspace with Undo; the keyboard and the header search work.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";
import { tmpPrefix } from "./runId";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

async function openUniversal(page: Page) {
	await page.getByRole("link", { name: "Universal" }).click();
	await expect(page.getByRole("heading", { name: "Universal", level: 1 })).toBeVisible();
}

const row = (page: Page, text: string) => page.locator("main.universal .row").filter({ hasText: text });

test("rows from a second workspace join the list, grouped by priority", async ({ page }) => {
	const second = mkdtempSync(join(tmpdir(), tmpPrefix("universal-")));
	writeFileSync(join(second, "todo.txt"), "(B) water the plants @home\n");
	try {
		await page.getByRole("button", { name: "Open workspace navigation" }).click();
		await page.getByPlaceholder("/path/to/workspace").fill(second);
		await page.getByRole("button", { name: "Add", exact: true }).click();
		await expect(page.getByRole("menu")).toContainText(second);
		await page.keyboard.press("Escape");

		await openUniversal(page);
		await expect(row(page, "buy milk")).toBeVisible();
		await expect(row(page, "water the plants")).toBeVisible();
		await expect(page.getByRole("region", { name: "(A)" })).toContainText("buy milk");
		await expect(page.getByRole("region", { name: "(B)" })).toContainText("water the plants");
	} finally {
		rmSync(second, { recursive: true, force: true });
	}
});

test("the round mark completes in the file, and Undo puts it back", async ({ page }) => {
	await openUniversal(page);
	await row(page, "call mum").getByRole("button", { name: "Mark done" }).click();
	await expect.poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8")).toMatch(/^x \d{4}-\d{2}-\d{2} .*call mum/m);
	await expect(row(page, "call mum")).toHaveCount(0); // show done is off

	await page.getByRole("button", { name: "Undo" }).click();
	await expect.poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8")).toMatch(/^call mum @phone$/m);
	await expect(row(page, "call mum")).toBeVisible();
});

test("the header search filters rows, Enter hands the keyboard to them, and x completes", async ({ page }) => {
	await openUniversal(page);
	const search = page.getByRole("textbox", { name: "Search" });
	await search.fill("mum");
	await expect(page.getByRole("status", { name: "Matching lines" })).toHaveText("1");
	await expect(row(page, "buy milk")).toHaveCount(0);

	await search.press("Enter");
	const open = row(page, "call mum").locator("button.open");
	await expect(open).toBeFocused();
	await page.keyboard.press("x");
	await expect.poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8")).toMatch(/^x \d{4}-\d{2}-\d{2} .*call mum/m);
});
