// Esc in an editor (2026-10-06 feedback): the editor first does its own Esc (discard an unsaved
// edit), then lets go of the keyboard, so the single-key shortcuts (`/`, `g t`, `?`) work again,
// and anything that closes on Esc (the detail panel) hears it in the same press. Editors call
// `leaveEditor`; closers subscribe with `onEditorEscape`.

const listeners = new Set<() => void>();

/** Blurs `el` (the editor's content) and tells every Esc listener. */
export function leaveEditor(el: HTMLElement | null | undefined): void {
	// HTMLElement.blur: https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/blur
	el?.blur();
	for (const fn of [...listeners]) fn();
}

/** Runs `fn` on every editor Esc; returns the unsubscribe. */
export function onEditorEscape(fn: () => void): () => void {
	listeners.add(fn);
	return () => listeners.delete(fn);
}
