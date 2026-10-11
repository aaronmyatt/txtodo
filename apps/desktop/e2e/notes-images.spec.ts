// tasks/enable-dragging-and-dropping-images-to-a (option A, decided 2026-10-11): a pasted image is
// saved beside notes.md under assets/, linked as markdown, and drawn under its line. The same
// handler takes a drop; a paste is the one a test can send without a real drag.
import { expect, test } from "@playwright/test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

// A 1×1 PNG.
const PNG =
	"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

test("a pasted image is saved beside notes.md, linked and shown", async ({ page }) => {
	await page.locator(".buffer .cm-content").getByText("plan the roadmap").dblclick();
	const notes = page.locator("section.notes .notes-editor .cm-content");
	await expect(notes).toContainText("Q4 goals");
	await notes.click();
	await page.keyboard.press("ControlOrMeta+End");
	await notes.evaluate((el, b64) => {
		const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
		const data = new DataTransfer();
		data.items.add(new File([bytes], "dot.png", { type: "image/png" }));
		el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
	}, PNG);

	const assets = join(daemon.dir, "tasks", "q4-roadmap", "assets");
	await expect.poll(() => (existsSync(assets) ? readdirSync(assets) : [])).toEqual([expect.stringMatching(/^[0-9a-f]{8}-dot\.png$/)]);
	const notesFile = join(daemon.dir, "tasks", "q4-roadmap", "notes.md");
	await expect.poll(() => readFileSync(notesFile, "utf8")).toMatch(/!\[dot\]\(assets\/[0-9a-f]{8}-dot\.png\)/);
	await expect(page.locator("section.notes img.cm-note-image")).toHaveAttribute("src", /^blob:/);
});
