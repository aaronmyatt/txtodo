// tasks/desktop-ui-revamp, accessibility pass: every control on each page has a name, the header
// search shows a ring while it has the keyboard, the nav marks the current page, and the hit count
// is a polite live region. Contrast is checked from app.css in src/lib/contrast.test.ts.
import { expect, test, type Page } from "@playwright/test";
import { type DaemonHandle, spawnDaemon } from "./fixtures";
import { gotoWithDaemon } from "./helpers";

let daemon: DaemonHandle;

test.beforeEach(async ({ page }) => {
	daemon = await spawnDaemon("todo");
	await gotoWithDaemon(page, daemon);
	await expect(page.locator(".buffer .cm-content")).toContainText("buy milk");
});

test.afterEach(() => {
	daemon.dispose();
});

/** Visible controls with no accessible name, by the plain rules (label, labelledby, text, title). */
function unnamed(page: Page): Promise<string[]> {
	return page.evaluate(() => {
		const name = (el: Element): string => {
			const label = el.getAttribute("aria-label");
			if (label) return label;
			const by = el.getAttribute("aria-labelledby");
			if (by) return by.split(" ").map((id) => document.getElementById(id)?.textContent ?? "").join(" ").trim();
			if (el.id && document.querySelector(`label[for="${el.id}"]`)) return "label";
			if (el.closest("label")) return "label";
			return (el.textContent ?? "").trim() || el.getAttribute("title") || el.getAttribute("placeholder") || "";
		};
		const controls = document.querySelectorAll("button, a[href], input, select, textarea, [role=textbox], [role=button]");
		return [...controls].filter((el) => el.getClientRects().length > 0 && !name(el)).map((el) => el.outerHTML.slice(0, 120));
	});
}

test("every control on each page has a name, and the nav marks the page", async ({ page }) => {
	expect(await unnamed(page)).toEqual([]);
	for (const link of ["Universal", "Settings"]) {
		await page.getByRole("link", { name: link }).click();
		await expect(page.getByRole("link", { name: link })).toHaveAttribute("aria-current", "page");
		await page.waitForLoadState("networkidle");
		expect(await unnamed(page)).toEqual([]);
	}
});

test("the header search shows a ring with the keyboard, and its count is a polite live region", async ({ page }) => {
	await page.keyboard.press("/");
	const search = page.getByRole("textbox", { name: "Search" });
	await expect(search).toBeFocused();
	const ring = await page.locator("label.search").evaluate((el) => getComputedStyle(el).boxShadow);
	expect(ring).not.toBe("none");
	await search.fill("milk");
	await expect(page.getByRole("status", { name: "Matching lines" })).toHaveAttribute("aria-live", "polite");
});
