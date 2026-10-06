// What the status footer says about the main buffer (task desktop-ui-revamp): its save state and
// the caret's line and column. The root FileView reports here (`reportStatus`); nested ones don't.
import { writable } from "svelte/store";
import type { DocCounts } from "$lib/todotxt/counts";

/** The buffer's save state: matches the daemon, has unsaved typing, is being saved, was just
 * saved, or the last save was refused. */
export type SaveState = "synced" | "unsaved" | "saving" | "saved" | "refused";

export interface EditorStatus {
	save: SaveState;
	/** The buffer's open tasks, done tasks and lines (`$lib/todotxt/counts`). */
	counts: DocCounts;
	/** The header search over this buffer: matching lines, and which of them is current (1-based,
	 * 0 before the first Enter). */
	search: { hits: number; index: number };
	/** 1-based caret line and column; 0 when no buffer is showing. */
	line: number;
	col: number;
}

export const EMPTY_STATUS: EditorStatus = {
	save: "synced",
	counts: { open: 0, done: 0, lines: 0 },
	search: { hits: 0, index: 0 },
	line: 0,
	col: 0
};

export const editorStatus = writable<EditorStatus>(EMPTY_STATUS);

/** How long "saved" shows before it settles back to "synced". */
export const SAVED_MS = 1500;

/** The footer's words for a save state. */
export const SAVE_LABEL: Record<SaveState, string> = {
	synced: "synced",
	unsaved: "unsaved",
	saving: "saving…",
	saved: "saved",
	refused: "not saved"
};
