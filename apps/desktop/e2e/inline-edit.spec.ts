// tasks/desktop-playwright-tests (superseded "raw mode" stretch, plan §3.2/§7): the document is
// always a live, editable buffer — no separate mode to toggle. Real `txtodod` behind the
// test-only `e2e_bridge` (see tasks/desktop-playwright-tests/notes.md for the harness) — `apply`
// is the exact RPC the conflict-review sheet already uses, so these scenarios exercise the real
// `Apply` path and a real `needs_review` flag, never mocks. `ControlOrMeta` is Playwright's own
// cross-platform stand-in for CM6's "Mod" keymap convention (Cmd on macOS, Ctrl elsewhere):
// https://playwright.dev/docs/api/class-keyboard.
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

test("editing a line and pressing Cmd/Ctrl-S commits it immediately", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");

	const content = page.locator(".editor-shell .cm-content");
	// The root view stretches its editor to fill the window (`fill` mode), so a plain click on the
	// whole (mostly empty) content box can land below the visible text once the doc has loaded —
	// and clicking the line by its expected text also waits out the initial async load itself.
	await page.locator(".cm-line", { hasText: "call mum" }).first().click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +home");
	await page.keyboard.press("ControlOrMeta+s");

	await expect.poll(() => readFileSync(todoPath, "utf8")).toContain("+home");
	const afterLines = readFileSync(todoPath, "utf8").trim().split("\n");
	expect(afterLines).toHaveLength(1); // a delta, not a whole-file replace: still one line
	expect(afterLines[0]).toContain("id:01ARZ3NDEKTSV4RRFFQ69G5FAV"); // same task, same id
});

test("Escape discards an in-progress edit without ever writing the file", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");
	const before = readFileSync(todoPath, "utf8");

	const content = page.locator(".editor-shell .cm-content");
	await page.locator(".cm-line", { hasText: "call mum" }).first().click();
	await page.keyboard.press("End");
	await page.keyboard.type(" scratch, discard me");
	await page.keyboard.press("Escape");

	await expect(content).not.toContainText("discard me");
	await expect(content).toContainText("call mum");

	// No `Apply` call was ever made, so there's nothing to poll for — assert the negative directly
	// after giving the (nonexistent) write a moment it would have needed to land.
	await page.waitForTimeout(300);
	expect(readFileSync(todoPath, "utf8")).toBe(before);
});

test("blur commits an in-progress edit", async ({ page }) => {
	daemon = await spawnDaemon("popover");
	await gotoWithDaemon(page, daemon);
	const todoPath = join(daemon.dir, "todo.txt");

	const content = page.locator(".editor-shell .cm-content");
	await page.locator(".cm-line", { hasText: "call mum" }).first().click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +errand");

	// Focus something outside the CM6 editor entirely.
	await page.locator(".add-line-row input").click();

	await expect.poll(() => readFileSync(todoPath, "utf8")).toContain("+errand");
});

test("a concurrent external edit during an in-progress edit surfaces the conflict banner, not a silent overwrite", async ({
	page
}) => {
	daemon = await spawnDaemon("conflict");
	await gotoWithDaemon(page, daemon);

	const content = page.locator(".editor-shell .cm-content");
	await page.locator(".cm-line", { hasText: "buy milk" }).first().click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +errand");

	// A concurrent edit lands while this buffer is still dirty — same substitute conflict.spec.ts
	// uses for "a second daemon" (see that file's module doc): raising the flag directly in the
	// op-log store, standing in for the sync transport that isn't wired up yet.
	await debugRaiseConflict(daemon, {
		path: "todo.txt",
		taskId: CONFLICT_TASK_ID,
		mine: CONFLICT_MINE,
		theirs: CONFLICT_THEIRS
	});

	const banner = page.getByRole("status").filter({ hasText: "need review" });
	await expect(banner).toBeVisible();

	// The concurrent change did not silently overwrite the human's in-progress edit.
	await expect(content).toContainText("+errand");

	await banner.getByRole("button", { name: "Review" }).click();
	const sheet = page.getByRole("dialog", { name: /review conflicting edit/i });
	await expect(sheet).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep mine" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep theirs" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep merged" })).toBeVisible();
});

test("navigating away (breadcrumb Home) commits a dirty buffer before the view unmounts", async ({
	page
}) => {
	daemon = await spawnDaemon("nested");
	await gotoWithDaemon(page, daemon);
	const subListPath = join(daemon.dir, "q4-roadmap", "todo.txt");

	await page.locator(".cm-line", { hasText: "plan the roadmap" }).first().dblclick();
	const subListContent = page.locator("section.sublist .editor-shell .cm-content");
	await expect(page.locator("section.sublist .cm-line", { hasText: "draft the outline" })).toBeVisible();

	await subListContent.click();
	await page.keyboard.press("End");
	await page.keyboard.type(" +planning");

	// Never explicitly commit — navigate straight back to the root view instead, unmounting this
	// `FileView` (and the whole `DetailView`) with the buffer still dirty.
	await page.getByRole("button", { name: "Home" }).click();
	await expect(page.locator("section.sublist")).toHaveCount(0);

	await expect.poll(() => readFileSync(subListPath, "utf8")).toContain("+planning");
});
