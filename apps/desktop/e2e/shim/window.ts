// Test-only stand-in for `@tauri-apps/api/window` (tasks/desktop-playwright-tests/notes.md),
// active only under `vite dev --mode e2e` (see vite.config.ts's alias and ./core.ts's module doc).
// Only `getCurrentWindow().label` is used outside a real Tauri runtime today
// (src/routes/+page.svelte's quick-add-vs-main check); none of the six Playwright scenarios drive
// quick-add, so this always reports the main window and no-ops everything else.

interface ShimWindow {
	label: string;
	hide: () => Promise<void>;
	show: () => Promise<void>;
	setFocus: () => Promise<void>;
}

const mainWindow: ShimWindow = {
	label: "main",
	hide: async () => {},
	show: async () => {},
	setFocus: async () => {}
};

/** Mirrors `@tauri-apps/api/window`'s `getCurrentWindow`: https://v2.tauri.app/reference/javascript/api/namespacewindow/ */
export function getCurrentWindow(): ShimWindow {
	return mainWindow;
}
