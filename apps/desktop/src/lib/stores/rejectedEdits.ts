// Edits the daemon refused, kept so the human can get their text back (root todo id:
// 01M2WK5DQQ9A3V3M4F2CJN0KMX). FileView commits its buffer on blur, on Cmd-S, on a path switch and
// on unmount; the last two have no editor left to show the buffer or its error in, so a refusal
// there used to drop the text on the floor. Every refusal lands here instead, and
// `RejectedEditBanner` (mounted once in MainView) shows it with a copy button.
//
// One entry per path: a newer refusal for the same file replaces the older text, and a later
// successful commit of that file clears it.
//
// Relative imports only, so `rejectedEdits.test.ts` runs under the plain-Node vitest config (see
// pin.ts's own note on why `$lib` does not resolve there).
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { writable } from "svelte/store";

export interface RejectedEdit {
	/** Workspace-relative path of the file the edit was aimed at. */
	path: string;
	/** The buffer text the human typed, exactly as it was when the daemon refused it. */
	text: string;
	/** The daemon's reason, as shown to the human. */
	error: string;
}

const { subscribe, update, set } = writable<RejectedEdit[]>([]);

export const rejectedEdits = {
	subscribe,
	/** Keeps `edit` for its path, replacing any earlier refusal of the same file. */
	record(edit: RejectedEdit): void {
		update((all) => [...all.filter((e) => e.path !== edit.path), edit]);
	},
	/** Drops the entry for `path` (the human dismissed it, or a later commit of it succeeded). */
	clear(path: string): void {
		update((all) => all.filter((e) => e.path !== path));
	},
	/** Test seam: back to empty. */
	reset(): void {
		set([]);
	}
};
