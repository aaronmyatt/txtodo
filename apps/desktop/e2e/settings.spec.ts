// tasks/desktop-ui-revamp: Settings. The nav lists the cards and follows the reader, a deep link
// lands on its card, the header search hides cards, and a preference applies at once.
import { expect, test } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
	await page.getByRole("link", { name: "Settings" }).click();
});

test.afterEach(() => {
	daemon.dispose();
});

const nav = (page: import("@playwright/test").Page) => page.getByRole("navigation", { name: "Settings sections" });

test("seven cards, and a nav click marks its card current", async ({ page }) => {
	await expect(nav(page).getByRole("button")).toHaveCount(7);
	await nav(page).getByRole("button", { name: "Tokens" }).click();
	await expect(nav(page).getByRole("button", { name: "Tokens" })).toHaveAttribute("aria-current", "true");
	await expect(page.locator("section#tokens")).toBeInViewport();
});

test("the header search hides cards that miss, with the count in the pill", async ({ page }) => {
	await page.getByRole("textbox", { name: "Search" }).fill("theme");
	await expect(page.getByRole("status", { name: "Matching lines" })).toHaveText("1");
	await expect(page.locator("section#appearance")).toBeVisible();
	await expect(page.locator("section#tokens")).toHaveCount(0);
	await page.getByRole("textbox", { name: "Search" }).fill("zzzz");
	await expect(page.getByText("No settings match that search.")).toBeVisible();
});

test("the theme applies at once and line numbers follow the toggle", async ({ page }) => {
	await page.locator("section#appearance").getByRole("button", { name: "Dark" }).click();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
	await page.locator("section#appearance").getByRole("button", { name: "Light" }).click();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

	await page.getByLabel("Line numbers").uncheck();
	await expect(page.locator("section#appearance .cm-lineNumbers")).toHaveCount(0);
});
