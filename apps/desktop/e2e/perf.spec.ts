// tasks/desktop-visual-regression, plan M7 acceptance: "Startup to first paint of a 10k-line file
// ≤ 500 ms on the CI runner." Loads the same shared fixture the main-view snapshot uses
// (./tenKFixture.ts — "one fixture, two consumers", notes.md) through a real daemon, and asserts
// the measured budget.
//
// FROZEN-PATH NOTE: the ≤500ms number below is meant to become a named `budgets.json` perf key
// (notes.md: "a constitution number... gets a named check, never a comment-only promise"), but
// `.claude/budgets.json` is a frozen path requiring an explicit human-reviewed `/setup` write —
// different from most frozen paths in this repo, and NOT something this session may touch or
// propose a diff for. Hard-coding the constant here, with this comment, is the deliberate
// stand-in until a human runs `/setup` to promote it (see this task's notes.md "As built" entry
// for the full flag).
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";
import { TEN_K_DOC_LINES } from "./tenKFixture";

/** See the FROZEN-PATH NOTE above: promote to `.claude/budgets.json` via a human-reviewed
 * `/setup` pass, not by this task editing that file directly. */
const FIRST_PAINT_BUDGET_MS = 500;

test("10k-line first paint stays within the perf budget", async ({ page }) => {
	// Warm-up navigation on a tiny, separate fixture first. This is a dev-server (`vite dev`)
	// harness, so a "cold" first navigation pays Vite's on-demand module-compile cost — a
	// dev-only artifact absent from the shipped, pre-built Tauri app, and not what this budget is
	// about (notes.md: "the actual risk is eager decoration/tokenization of the full 10k lines").
	// Warming the module graph up first isolates the number that IS the point of this test: the
	// incremental cost of the document being 10k lines, not of Vite compiling for the first time.
	const warmup = await spawnDaemon("todo");
	await gotoWithDaemon(page, warmup);
	await expect(page.locator("[data-line-count]")).not.toHaveAttribute("data-line-count", "0");
	warmup.dispose();

	const daemon: DaemonHandle = await spawnDaemon("ten-k");
	try {
		const startedMs = Date.now();
		await gotoWithDaemon(page, daemon);
		await page.waitForSelector(`[data-line-count='${TEN_K_DOC_LINES}']`);
		const paintMs = Date.now() - startedMs;

		// eslint-disable-next-line no-console -- perf numbers belong in the test log, not hidden
		console.log(`10k-line first paint: ${paintMs}ms (budget ${FIRST_PAINT_BUDGET_MS}ms)`);
		expect(paintMs).toBeLessThanOrEqual(FIRST_PAINT_BUDGET_MS);
	} finally {
		daemon.dispose();
	}
});
