// Test-only stand-in for `@tauri-apps/api/window` (tasks/desktop-playwright-tests/notes.md),
// active only under `vite dev --mode e2e` (see vite.config.ts's alias and ./core.ts's module doc).
// Only `getCurrentWindow().label` is used outside a real Tauri runtime today
// (the layout's quick-add-vs-main check). `?window=quick-add` in the page URL reports the quick-add
// window instead, so a spec can drive that window's UI; everything else no-ops. This file is only
// aliased in under `--mode e2e`, so the seam never ships.

interface ShimWindow {
	label: string;
	hide: () => Promise<void>;
	show: () => Promise<void>;
	setFocus: () => Promise<void>;
}

// URLSearchParams: https://developer.mozilla.org/en-US/docs/Web/API/URLSearchParams
const label = new URLSearchParams(location.search).get("window") === "quick-add" ? "quick-add" : "main";

const mainWindow: ShimWindow = {
	label,
	hide: async () => {},
	show: async () => {},
	setFocus: async () => {}
};

/** Mirrors `@tauri-apps/api/window`'s `getCurrentWindow`: https://v2.tauri.app/reference/javascript/api/namespacewindow/ */
export function getCurrentWindow(): ShimWindow {
	return mainWindow;
}
