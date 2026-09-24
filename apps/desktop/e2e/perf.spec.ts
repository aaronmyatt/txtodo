// tasks/desktop-visual-regression, plan M7 acceptance: "Startup to first paint of a 10k-line file
// ≤ 500 ms on the CI runner." Loads the same shared fixture the main-view snapshot uses
// (./tenKFixture.ts — "one fixture, two consumers", notes.md) through a real daemon, and asserts
// the measured budget.
//
// The budget itself lives in `.claude/budgets.json` as `perf.firstPaint10kMs` (decided 2026-09-24),
// so it is a named number next to the other perf budgets, not a comment-only promise.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";
import { TEN_K_DOC_LINES } from "./tenKFixture";

/** `.claude/budgets.json` `perf.firstPaint10kMs`, read at load so the spec and the budget can't
 * drift. Ref: https://nodejs.org/api/fs.html#fsreadfilesyncpath-options */
const BUDGETS = join(dirname(fileURLToPath(import.meta.url)), "../../../.claude/budgets.json");
const FIRST_PAINT_BUDGET_MS: number = JSON.parse(readFileSync(BUDGETS, "utf8")).perf.firstPaint10kMs;
if (typeof FIRST_PAINT_BUDGET_MS !== "number") {
	throw new Error(`${BUDGETS} has no numeric perf.firstPaint10kMs`);
}

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
