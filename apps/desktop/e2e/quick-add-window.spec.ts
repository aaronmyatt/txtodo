// tasks/desktop-ui-revamp: the Quick Add window is the prompt bar in its windowed form. The e2e
// window shim reports the quick-add label for `?window=quick-add` (e2e/shim/window.ts), so this
// drives that window's UI against the real daemon.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
  daemon = await spawnDaemon("todo");
  await gotoWithDaemon(page, daemon, "/?window=quick-add");
});

test.afterEach(() => {
  daemon.dispose();
});

test("the Quick Add window has the keyboard, shows its chips and adds on Enter", async ({
  page,
}) => {
  const field = page.getByRole("textbox", { name: "Add a task" });
  await expect(field).toBeFocused();
  // No app header in this window, and the chips sit in the flow without focus games.
  await expect(page.getByRole("textbox", { name: "Search" })).toHaveCount(0);
  await expect(
    page.getByRole("toolbar", { name: "Insert a token" }),
  ).toBeVisible();

  await page.keyboard.type("water the plants @home");
  await page.keyboard.press("Enter");
  await expect
    .poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8"))
    .toMatch(/^.*water the plants @home\n/);
});
