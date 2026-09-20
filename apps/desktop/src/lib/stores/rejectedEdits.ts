// Edits the daemon refused, kept so the human can get their text back (root todo id:
// 01M2WK5DQQ9A3V3M4F2CJN0KMX). FileView commits its buffer on blur, on Cmd-S, on a path switch and
// on unmount; the last two have no editor left to show the buffer or its error in, so a refusal
// there used to drop the text on the floor. Every refusal lands here instead, and
// `RejectedEditBanner` (mounted once in MainView) shows it with a copy button.
//
// One entry per workspace and path: a newer refusal for the same file replaces the older text,
// and a later successful commit of that file clears it. Path alone is not enough (code review
// 2026-09-20, finding 4): every workspace has a `todo.txt`, so a good save in workspace B used to
// clear the text workspace A's refusal was holding.
//
// Relative imports only, so `rejectedEdits.test.ts` runs under the plain-Node vitest config (see
// pin.ts's own note on why `$lib` does not resolve there).
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { writable } from "svelte/store";

export interface RejectedEdit {
	/** Absolute root of the workspace the buffer was read under (`""` when none was selected). */
	workspace: string;
	/** Workspace-relative path of the file the edit was aimed at. */
	path: string;
	/** The buffer text the human typed, exactly as it was when the daemon refused it. */
	text: string;
	/** The daemon's reason, as shown to the human. */
	error: string;
}

const { subscribe, update, set } = writable<RejectedEdit[]>([]);

function sameFile(edit: RejectedEdit, workspace: string, path: string): boolean {
	return edit.workspace === workspace && edit.path === path;
}

export const rejectedEdits = {
	subscribe,
	/** Keeps `edit` for its path, replacing any earlier refusal of the same file. */
	record(edit: RejectedEdit): void {
		update((all) => [...all.filter((e) => !sameFile(e, edit.workspace, edit.path)), edit]);
	},
	/** Drops the entry for this workspace's `path` (dismissed, or a later commit of it succeeded). */
	clear(workspace: string, path: string): void {
		update((all) => all.filter((e) => !sameFile(e, workspace, path)));
	},
	/** Test seam: back to empty. */
	reset(): void {
		set([]);
	}
};
