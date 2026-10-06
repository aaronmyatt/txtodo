// tasks/desktop-ui-revamp: the prompt bar at the foot of the window (Quick Add on every screen).
// Enter adds to the open workspace's root list through the real daemon, the bar keeps the
// keyboard for the next line, and the chips float above it only while it has the keyboard.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
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

test("Enter in the prompt bar adds the line and keeps the keyboard", async ({
  page,
}) => {
  const field = page.getByRole("textbox", { name: "Add a task" });
  await expect(
    page.getByRole("toolbar", { name: "Insert a token" }),
  ).toHaveCount(0);
  await field.click();
  await expect(
    page.getByRole("toolbar", { name: "Insert a token" }),
  ).toBeVisible();
  await page
    .getByRole("toolbar", { name: "Insert a token" })
    .getByRole("button", { name: "(B)" })
    .click();
  await page.keyboard.type("call the bank");
  await page.keyboard.press("Enter");

  // At the top of the list (2026-10-06 feedback), not appended at the end.
  await expect
    .poll(() => readFileSync(join(daemon.dir, "todo.txt"), "utf8"))
    .toMatch(/^\(B\) .*call the bank\n/);
  await expect(field).toBeFocused();
  // CodeMirror shows its placeholder only while the document is empty
  // (https://codemirror.net/docs/ref/#view.placeholder), so it stands in for "the field is empty".
  await expect(field.locator(".cm-placeholder")).toBeVisible();
});

test("the mark is a live meter of the root list's done over total", async ({
  page,
}) => {
  const mark = page.locator(".prompt-mark title");
  await expect(mark).toHaveText(/^0 of \d+ done$/);
  const before = Number((await mark.textContent())?.match(/of (\d+)/)?.[1]);
  await page.getByRole("textbox", { name: "Add a task" }).click();
  await page.keyboard.type("one more");
  await page.keyboard.press("Enter");
  await expect(mark).toHaveText(`0 of ${before + 1} done`);
});
