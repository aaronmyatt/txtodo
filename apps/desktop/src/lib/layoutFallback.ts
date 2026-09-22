// What to tell the human when the `workspace_layout` call fails (task desktop-notes-hidden):
// MainView used to swallow it and silently assume `tasks/`, so an older daemon that resolves
// `<slug>/notes.md` beside the list and a client composing `tasks/<slug>` disagreed with nothing
// saying so. Pure: no DOM, no Tauri, unit tested in `__tests__/layoutFallback.test.ts`.

/** The banner text for a failed layout fetch; `err` is the bridge's error string. */
export function layoutFallbackMessage(err: string): string {
	const detail = err.trim() ? ` (${err.trim()})` : "";
	return (
		`The running daemon did not answer the workspace layout call${detail}. ` +
		"It is likely an older build than this app: sub-lists and notes are shown from `tasks/<slug>` " +
		"until it is updated. Run `txtodo daemon install` then `txtodo daemon start`, or reinstall the app."
	);
}
