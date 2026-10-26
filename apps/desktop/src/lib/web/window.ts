// Stand-in for `@tauri-apps/api/window` in the web build (vite.config.ts's alias; ADR 0042): a
// browser tab is always the main window, and hiding, showing or pinning it is not the page's to do.
// Ref: https://v2.tauri.app/reference/javascript/api/namespacewindow/#getcurrentwindow

interface WebWindow {
	label: string;
	hide: () => Promise<void>;
	show: () => Promise<void>;
	setFocus: () => Promise<void>;
	setAlwaysOnTop: (on: boolean) => Promise<void>;
}

const tab: WebWindow = {
	label: "main",
	hide: async () => {},
	show: async () => {},
	setFocus: async () => {},
	setAlwaysOnTop: async () => {}
};

/** Mirrors Tauri's `getCurrentWindow`. */
export function getCurrentWindow(): WebWindow {
	return tab;
}
