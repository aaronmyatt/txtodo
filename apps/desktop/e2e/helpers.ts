// Shared per-spec setup: points the page under test at one spawned daemon's `e2e_bridge` before
// any app code runs (`page.addInitScript` runs before the page's own scripts — see
// https://playwright.dev/docs/api/class-page#page-add-init-script — so `e2e/shim/core.ts` reads
// `window.__E2E_BRIDGE_URL__` on its very first `invoke` call, never a stale default).
import type { Page } from "@playwright/test";
import type { DaemonHandle } from "./fixtures";

export async function gotoWithDaemon(page: Page, daemon: DaemonHandle, path = "/"): Promise<void> {
	await page.addInitScript((port: number) => {
		(window as unknown as { __E2E_BRIDGE_URL__?: string }).__E2E_BRIDGE_URL__ =
			`http://127.0.0.1:${port}`;
	}, daemon.port);
	await page.goto(path);
}
