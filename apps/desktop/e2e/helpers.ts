// Shared per-spec setup: points the page under test at one spawned daemon's `e2e_bridge` before
// any app code runs (`page.addInitScript` runs before the page's own scripts — see
// https://playwright.dev/docs/api/class-page#page-add-init-script — so `e2e/shim/core.ts` reads
// `window.__E2E_BRIDGE_URL__` on its very first `invoke` call, never a stale default).
import { expect, type Locator, type Page } from "@playwright/test";
import type { DaemonHandle } from "./fixtures";

export async function gotoWithDaemon(page: Page, daemon: DaemonHandle, path = "/"): Promise<void> {
	await page.addInitScript((port: number) => {
		(window as unknown as { __E2E_BRIDGE_URL__?: string }).__E2E_BRIDGE_URL__ =
			`http://127.0.0.1:${port}`;
	}, daemon.port);
	await page.goto(path);
}

/**
 * Reveals and clicks a line's hover pencil (FileView.svelte). The pencil only exists while
 * `hoveredLine` is truthy, which CM6's own `mousemove` handler recomputes on every real pointer
 * move it sees — including the intermediate moves a normal `locator.click()` synthesizes while
 * walking the mouse across the editor to reach the target, which can race the pencil right back
 * out from under the click. `dispatchEvent("click")` fires the click directly on the (already
 * revealed, already located) element without any further synthetic pointer movement, sidestepping
 * that race entirely — a real user's click doesn't have this problem because their pointer isn't
 * being re-driven across the editor between the hover and the click.
 */
export async function openPopoverFor(page: Page, lineLocator: Locator): Promise<void> {
	await lineLocator.hover();
	const pencil = page.locator(".pencil");
	await expect(pencil).toBeVisible();
	await pencil.dispatchEvent("click");
	await expect(page.locator(".popover")).toBeVisible();
}
