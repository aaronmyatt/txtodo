// tasks/desktop-playwright-tests, plan §3.2.4 lazy creation: "typing into empty notes creates the
// directory and adds the ref: tag in exactly one op batch." Also asserts the negative: opening the
// detail view alone writes nothing (notes.md: "notes creation asserts the negative too").
import { expect, test } from "@playwright/test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("notes-create");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

/** Anything in the workspace root besides the seeded todo.txt and the daemon's own `.txtodo/`. */
function newTopLevelEntries(): string[] {
	return readdirSync(daemon.dir).filter((f) => f !== "todo.txt" && f !== ".txtodo");
}

test("first keystroke into empty notes lazily creates the ref: directory", async ({ page }) => {
	const line = page.locator(".cm-line", { hasText: "plan the roadmap" }).first();
	await line.dblclick();

	const notesEditor = page.locator("section.notes .notes-editor .cm-content");
	await expect(notesEditor).toBeVisible();

	// Negative: opening the detail view alone must not have created anything yet.
	expect(newTopLevelEntries()).toEqual([]);

	await notesEditor.click();
	await page.keyboard.type("first note");

	// NotesEditor.svelte debounces its save 500ms — poll a real condition, not a fixed sleep
	// (notes.md: "conflict injection must be deterministic", the same principle applies here).
	await expect.poll(() => newTopLevelEntries().length, { timeout: 5000 }).toBe(1);

	const slug = newTopLevelEntries()[0];
	const notesPath = join(daemon.dir, slug, "notes.md");
	await expect.poll(() => existsSync(notesPath)).toBe(true);
	expect(readFileSync(notesPath, "utf8")).toBe("first note");

	// The parent line on disk now carries a ref: tag pointing at the new directory (one op batch,
	// server-side — crates/txtodo-daemon/src/refdir_ops.rs::ensure_ref_dir).
	const parentText = readFileSync(join(daemon.dir, "todo.txt"), "utf8");
	expect(parentText).toContain(`ref:${slug}`);
});
