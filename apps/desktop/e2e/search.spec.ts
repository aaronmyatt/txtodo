// tasks/desktop-ui-revamp: the header search on Tasks. Matching lines keep their place and get
// their hits marked, the rest fade; the pill counts, Enter steps (focus stays in the box), Esc
// clears; `is:done` and `-word` work as the core matches them.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("done");
	await gotoWithDaemon(page, daemon);
	await expect(page.locator(".buffer .cm-content")).toContainText("call mum");
});

test.afterEach(() => {
	daemon.dispose();
});

test("hits are marked, misses fade, the pill counts and Enter steps", async ({ page }) => {
	const search = page.getByRole("textbox", { name: "Search" });
	const pill = page.getByRole("status", { name: "Matching lines" });
	await search.fill("mum");
	await expect(pill).toHaveText("1");
	await expect(page.locator(".buffer .cm-hit")).toHaveText("mum");
	await expect(page.locator(".buffer .cm-line.cm-dim")).toContainText("pay rent");

	await search.press("Enter");
	await expect(pill).toHaveText("1/1");
	await expect(search).toBeFocused();

	await search.press("Escape");
	await expect(search).toHaveValue("");
	await expect(page.locator(".buffer .cm-hit")).toHaveCount(0);
});

test("is:done and a leading dash match as the core does", async ({ page }) => {
	const search = page.getByRole("textbox", { name: "Search" });
	const line = (text: string) => page.locator(".buffer .cm-line").filter({ hasText: text });
	await search.fill("is:done");
	await expect(line("call mum")).toHaveClass(/cm-dim/);
	await expect(line("pay rent")).not.toHaveClass(/cm-dim/);
	await search.fill("-rent");
	await expect(line("pay rent")).toHaveClass(/cm-dim/);
	await expect(line("call mum")).not.toHaveClass(/cm-dim/);
	await expect(page.getByRole("status", { name: "Matching lines" })).toHaveText("1");
});
