// tasks/desktop-raw-mode, plan §3.2/§7 "stretch: raw mode Cmd/Ctrl+E through the reconciler."
// Real `txtodod` behind the test-only `e2e_bridge` (see tasks/desktop-playwright-tests/notes.md
// for the harness) — `apply` is the exact RPC the edit popover and conflict resolution already
// use, so these scenarios exercise the real `Apply` path and a real `needs_review` flag, never
// mocks. `ControlOrMeta` is Playwright's own cross-platform stand-in for CM6's "Mod" keymap
// convention (Cmd on macOS, Ctrl elsewhere): https://playwright.dev/docs/api/class-keyboard.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
	CONFLICT_MINE,
	CONFLICT_TASK_ID,
	CONFLICT_THEIRS,
	type DaemonHandle,
	debugRaiseConflict,
	spawnDaemon
} from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.afterEach(() => {
	daemon.dispose();
});

/** The raw-mode toggle button (`FileView.svelte`'s header) — text flips between "Raw mode" and
 * "Raw mode: on", so match on the common prefix rather than either exact string. */
function rawBadge(page: import("@playwright/test").Page) {
	return page.getByRole("button", { name: /raw mode/i });
}

test("toggle raw mode, edit, save (Cmd/Ctrl-S), and see it reconciled", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");

	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "false");

	const content = page.locator(".editor-shell .cm-content");
	await content.click();
	await page.keyboard.press("ControlOrMeta+e");

	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "true");
	await expect(page.locator(".editor-shell .cm-editor[data-raw]")).toBeVisible();

	await page.keyboard.press("End");
	await page.keyboard.type(" +home");
	await page.keyboard.press("ControlOrMeta+s");

	// Commit flips the badge/border back immediately — it doesn't wait on the `Apply` round trip.
	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "false");
	await expect(page.locator(".editor-shell .cm-editor[data-raw]")).toHaveCount(0);

	await expect.poll(() => readFileSync(todoPath, "utf8")).toContain("+home");
	const afterLines = readFileSync(todoPath, "utf8").trim().split("\n");
	expect(afterLines).toHaveLength(1); // a delta, not a whole-file replace: still one line
	expect(afterLines[0]).toContain("id:01ARZ3NDEKTSV4RRFFQ69G5FAV"); // same task, same id
});

test("Esc discards the raw buffer without ever writing the file", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");
	const before = readFileSync(todoPath, "utf8");

	const content = page.locator(".editor-shell .cm-content");
	await content.click();
	await page.keyboard.press("ControlOrMeta+e");
	await page.keyboard.press("End");
	await page.keyboard.type(" scratch, discard me");
	await page.keyboard.press("Escape");

	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "false");
	await expect(content).not.toContainText("discard me");
	await expect(content).toContainText("call mum");

	// No `Apply` call was ever made, so there's nothing to poll for — assert the negative directly
	// after giving the (nonexistent) write a moment it would have needed to land.
	await page.waitForTimeout(300);
	expect(readFileSync(todoPath, "utf8")).toBe(before);
});

test("blur commits the raw buffer", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");

	const content = page.locator(".editor-shell .cm-content");
	await content.click();
	await page.keyboard.press("ControlOrMeta+e");
	await page.keyboard.press("End");
	await page.keyboard.type(" +errand");

	// Focus something outside the CM6 editor entirely (the header bar itself).
	await page.locator(".file-view-header").click();

	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "false");
	await expect.poll(() => readFileSync(todoPath, "utf8")).toContain("+errand");
});

test("a concurrent external edit during raw mode surfaces the conflict banner, not a silent overwrite", async ({
	page
}) => {
	daemon = await spawnDaemon("conflict");
	await gotoWithDaemon(page, daemon);

	const content = page.locator(".editor-shell .cm-content");
	await content.click();
	await page.keyboard.press("ControlOrMeta+e");
	await page.keyboard.press("End");
	await page.keyboard.type(" +errand");

	// A concurrent edit lands while raw mode is still open and dirty — same substitute
	// conflict.spec.ts uses for "a second daemon" (see that file's module doc): raising the flag
	// directly in the op-log store, standing in for the sync transport that isn't wired up yet.
	await debugRaiseConflict(daemon, {
		path: "todo.txt",
		taskId: CONFLICT_TASK_ID,
		mine: CONFLICT_MINE,
		theirs: CONFLICT_THEIRS
	});

	const banner = page.getByRole("status").filter({ hasText: "need review" });
	await expect(banner).toBeVisible();

	// The concurrent change did not silently overwrite the human's in-progress raw buffer.
	await expect(rawBadge(page)).toHaveAttribute("aria-pressed", "true");
	await expect(content).toContainText("+errand");

	await banner.getByRole("button", { name: "Review" }).click();
	const sheet = page.getByRole("dialog", { name: /review conflicting edit/i });
	await expect(sheet).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep mine" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep theirs" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep merged" })).toBeVisible();
});

test("navigating away (breadcrumb Home) commits a dirty raw buffer before the view unmounts", async ({
	page
}) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
	const subListPath = join(daemon.dir, "q4-roadmap", "todo.txt");

	await page.locator(".cm-line", { hasText: "plan the roadmap" }).first().dblclick();
	const subListContent = page.locator("section.sublist .editor-shell .cm-content");
	await expect(page.locator("section.sublist .cm-line", { hasText: "draft the outline" })).toBeVisible();

	await subListContent.click();
	await page.keyboard.press("ControlOrMeta+e");
	await page.keyboard.press("End");
	await page.keyboard.type(" +planning");

	// Never explicitly commit — navigate straight back to the root view instead, unmounting this
	// `FileView` (and the whole `DetailView`) with the raw buffer still dirty.
	await page.getByRole("button", { name: "Home" }).click();
	await expect(page.locator("section.sublist")).toHaveCount(0);

	await expect.poll(() => readFileSync(subListPath, "utf8")).toContain("+planning");
});
