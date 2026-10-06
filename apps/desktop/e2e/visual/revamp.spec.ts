// tasks/desktop-ui-revamp: goldens for the revamp's new surfaces, under both the light and dark
// projects: Universal, each Settings card, the Tasks page with a search active, the prompt bar's
// chips, and the Quick Add window. Paths, times and anything machine-specific are masked, so a
// golden only fails on a real visual change. Local only: CI does not run e2e/visual (ci.yml).
import { expect, test, type Page } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "../fixtures";
import { gotoWithDaemon } from "../helpers";

let daemon: DaemonHandle;

test.afterEach(() => {
	daemon.dispose();
});

async function open(page: Page, path = "/") {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon, path);
	await expect(page.getByText(/^Daemon:/)).toHaveCount(0);
}

test("Universal", async ({ page }) => {
	await open(page);
	await page.getByRole("link", { name: "Universal" }).click();
	await expect(page.locator("main.universal .row").first()).toBeVisible();
	// The workspace chip and tiles name a temp folder (the header's name is hidden by stable.css).
	await expect(page).toHaveScreenshot("universal.png", {
		mask: [page.locator(".tile"), page.getByRole("group", { name: "Workspaces" })]
	});
});

const CARDS = ["general", "appearance", "workspaces", "shortcuts", "devices", "tokens", "activity"];

test("Settings cards", async ({ page }) => {
	await open(page);
	await page.getByRole("link", { name: "Settings" }).click();
	for (const id of CARDS) {
		const card = page.locator(`section#${id}`);
		await card.scrollIntoViewIfNeeded();
		await expect(card).toHaveScreenshot(`settings-${id}.png`, {
			mask: [
				card.locator(".path, .seen, .time, .workspace-chip, .principal, .op, .hint, .name"),
				card.locator(".error, .state-error")
			]
		});
	}
});

test("Tasks with a search active", async ({ page }) => {
	await open(page);
	await expect(page.locator(".buffer .cm-content")).toContainText("call mum");
	await page.getByRole("textbox", { name: "Search" }).fill("milk");
	await expect(page.locator(".buffer .cm-hit")).toHaveText("milk");
	await expect(page).toHaveScreenshot("search-active.png");
});

test("prompt bar chips", async ({ page }) => {
	await open(page);
	await page.getByRole("textbox", { name: "Add a task" }).click();
	await expect(page.getByRole("toolbar", { name: "Insert a token" })).toBeVisible();
	await expect(page.locator(".prompt")).toHaveScreenshot("prompt-chips.png");
});

test("Quick Add window", async ({ page }) => {
	await page.setViewportSize({ width: 480, height: 150 });
	await open(page, "/?window=quick-add");
	await expect(page.getByRole("textbox", { name: "Add a task" })).toBeFocused();
	await expect(page).toHaveScreenshot("quick-add.png");
});
