// tasks/layout-hot-reload-clients: the daemon hot-reloads `txtodo.toml` while the app is open and
// announces it on the watch stream; the main view must switch to the new root list without a
// workspace switch or restart. The file is written straight to disk here, the way a sync or a
// hand edit would change it.
import { expect, test } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
});

test.afterEach(() => {
	daemon.dispose();
});

test("a txtodo.toml change on disk moves the main view to the new root list", async ({ page }) => {
	await expect(page.locator(".cm-line", { hasText: "buy milk" }).first()).toBeVisible();

	// The new list first, then the layout that names it: the daemon registers the file when the
	// layout lands, and the walker would not have found `work.txt` on its own.
	writeFileSync(join(daemon.dir, "work.txt"), "(A) from work.txt\n");
	writeFileSync(join(daemon.dir, "txtodo.toml"), 'todo_file = "work.txt"\n');

	await expect(page.locator(".cm-line", { hasText: "from work.txt" }).first()).toBeVisible({ timeout: 15000 });
	await expect(page.locator(".cm-line", { hasText: "buy milk" })).toHaveCount(0);
});
