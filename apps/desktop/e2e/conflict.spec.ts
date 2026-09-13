// tasks/desktop-playwright-tests, plan M7 acceptance: "conflict sheet appears when the test
// injects concurrent ops via a second daemon ... M4's three variants reachable."
//
// A real second-daemon-over-loopback injection is not reachable today: daemon-to-daemon sync has
// no transport wired up at all yet (`crates/txtodo-daemon/src/pairing_grpc.rs`'s own doc comment:
// "the leg that actually crosses between two daemons ... has no transport yet"; tracked as the
// still-blocked `sync-loopback-converge` in todo.txt). The daemon team's OWN integration tests hit
// the same wall and use the identical substitute this spec uses: raise the `needs_review` flag
// directly in the op-log store (`crates/txtodo-daemon/tests/grpc.rs::raise_flag`, doc comment:
// "what an import merge would do... no actual sync is needed") — see
// `e2e/fixtures.ts::debugRaiseConflict` and `e2e_bridge.rs::cmd_debug_raise_conflict`. Everything
// downstream of that flag (ListConflicts, the banner, the sheet, ResolveConflict, the on-disk
// write) is the real production path; only the "how the flag got raised" step is substituted, and
// by the same mechanism the daemon's own test suite already relies on.
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

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("conflict");
	await debugRaiseConflict(daemon, {
		path: "todo.txt",
		taskId: CONFLICT_TASK_ID,
		mine: CONFLICT_MINE,
		theirs: CONFLICT_THEIRS
	});
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("conflict banner and review sheet appear, and each resolution is reachable", async ({ page }) => {
	const banner = page.getByRole("status").filter({ hasText: "need review" });
	await expect(banner).toBeVisible();
	await expect(banner).toContainText("1");

	await banner.getByRole("button", { name: "Review" }).click();

	const sheet = page.getByRole("dialog", { name: /review conflicting edit/i });
	await expect(sheet).toBeVisible();
	// The sheet renders a merged interleave (reconstructs "theirs") plus a word-level diff, not
	// `mine`/`theirs` as two separate contiguous strings — assert the pieces the diff must show
	// rather than either original sentence verbatim (DiffView.svelte's `<ins>`/`<del>` markers).
	await expect(sheet.locator(".merged-preview")).toContainText("buy oat milk");
	await expect(sheet.locator(".diff-text ins")).toContainText("oat");

	await expect(sheet.getByRole("button", { name: "keep mine" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep theirs" })).toBeVisible();
	await expect(sheet.getByRole("button", { name: "keep merged" })).toBeVisible();
});

test("keep mine resolves the flag and writes mine's text back to the file", async ({ page }) => {
	const banner = page.getByRole("status").filter({ hasText: "need review" });
	await banner.getByRole("button", { name: "Review" }).click();

	const sheet = page.getByRole("dialog", { name: /review conflicting edit/i });
	await sheet.getByRole("button", { name: "keep mine" }).click();

	// The sheet auto-closes once its last flag resolves (ConflictReviewSheet.svelte's `$effect`).
	await expect(sheet).toHaveCount(0);
	await expect(page.getByRole("status").filter({ hasText: "need review" })).toHaveCount(0);

	await expect
		.poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8"))
		.toContain("buy milk");
});
