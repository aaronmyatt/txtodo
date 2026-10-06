// What the status footer says about the main buffer (task desktop-ui-revamp): its save state and
// the caret's line and column. The root FileView reports here (`reportStatus`); nested ones don't.
import { writable } from "svelte/store";

/** The buffer's save state: matches the daemon, has unsaved typing, is being saved, was just
 * saved, or the last save was refused. */
export type SaveState = "synced" | "unsaved" | "saving" | "saved" | "refused";

export interface EditorStatus {
	save: SaveState;
	/** 1-based caret line and column; 0 when no buffer is showing. */
	line: number;
	col: number;
}

export const editorStatus = writable<EditorStatus>({ save: "synced", line: 0, col: 0 });

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
