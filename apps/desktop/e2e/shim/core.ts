// Test-only stand-in for `@tauri-apps/api/core` (tasks/desktop-playwright-tests/notes.md), active
// only under `vite dev --mode e2e` (see vite.config.ts's alias). A plain browser has no Tauri IPC
// runtime, so this forwards every `invoke(cmd, args)` call to the `e2e_bridge` Rust binary's HTTP
// surface (apps/desktop/src-tauri/src/bin/e2e_bridge.rs) instead — same commands, same DTOs, same
// real daemon underneath, just JSON-over-HTTP instead of Tauri's IPC. The app's own code
// ($lib/daemon.ts and friends) is completely unaware of the swap: it still calls `invoke` from
// `@tauri-apps/api/core` and gets back the same shapes.
//
// `watch`/`daemon_status`/`retry_connect` are special-cased rather than forwarded — see
// `./event.ts`'s module doc for why the "watch" concept works differently under this harness.

const BRIDGE_URL = (globalThis as { __E2E_BRIDGE_URL__?: string }).__E2E_BRIDGE_URL__ ?? "http://127.0.0.1:4567";

/** Paths most recently passed to `watch(paths)`; `./event.ts`'s polling loop reads this. */
export const watchedPaths: string[] = [];

async function callBridge(cmd: string, args: Record<string, unknown>): Promise<unknown> {
	const res = await fetch(`${BRIDGE_URL}/invoke`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ cmd, args })
	});
	if (!res.ok) {
		throw new Error(await res.text());
	}
	const text = await res.text();
	return text ? JSON.parse(text) : undefined;
}

/** Mirrors `@tauri-apps/api/core`'s `invoke`: https://v2.tauri.app/develop/calling-rust/ */
export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
	switch (cmd) {
		case "daemon_status":
			return "connected" as T; // e2e_bridge only starts serving once a real daemon answered Health
		case "retry_connect":
			return "connected" as T;
		case "watch": {
			const paths = (args.paths as string[] | undefined) ?? [];
			watchedPaths.length = 0;
			watchedPaths.push(...paths);
			return undefined as T;
		}
		case "workspace_root":
			// tasks/desktop-visual-regression: forwarded to the bridge (see e2e_bridge.rs's
			// `invoke` doc comment) so DetailView.svelte's footer shows a real, non-empty
			// directory path — the six original scenarios never needed this, but a detail-view
			// golden with an empty footer wouldn't show what the acceptance criteria describe.
			return (await callBridge(cmd, args)) as T;
		case "set_main_popover_dirty":
			return undefined as T; // quick-add's guard isn't part of this harness's scenarios
		default:
			return (await callBridge(cmd, args)) as T;
	}
}
