// Which Tauri window loaded this page. One static build serves the main window and the hidden
// quick-add window (`src-tauri/src/quick_add.rs`); both the layout (no header) and the root page
// (QuickAdd instead of MainView) ask.
import { getCurrentWindow } from "@tauri-apps/api/window";

/** True in the quick-add window. `getCurrentWindow()` throws outside a Tauri webview
 * (https://v2.tauri.app/reference/javascript/api/namespacewindow/#getcurrentwindow), so a plain
 * browser is the main window, except that `npm run dev` honours `?window=quick-add` to preview
 * that window. `import.meta.env.DEV` is false in a build, so the seam never ships
 * (https://vite.dev/guide/env-and-mode#built-in-constants). */
export function isQuickAddWindow(): boolean {
	try {
		return getCurrentWindow().label === "quick-add";
	} catch {
		return import.meta.env.DEV && new URLSearchParams(location.search).get("window") === "quick-add";
	}
}
