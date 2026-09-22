// Which sections the detail view renders and how (task desktop-notes-hidden): pure decisions the
// Svelte branches in `DetailView.svelte` read, so they are unit testable without mounting the
// component (`__tests__/detailSections.test.ts`). The rule: the sub-list shows whenever its
// `todo.txt` has tasks, the notes always show, and when both show the notes collapse under the
// sub-list (open when they have text) so a long list never hides them.

/** The sub-list section: the real list, or the one add-line input that starts one. */
export type SubListMode = "tasks" | "start";

export function subListMode(subListTotal: number): SubListMode {
	return subListTotal > 0 ? "tasks" : "start";
}

/** How the notes section sits: collapsible under a sub-list, plain otherwise; and whether a
 * collapsible one starts open (only when notes.md has text). */
export interface NotesLayout {
	collapsible: boolean;
	open: boolean;
}

export function notesLayout(subListTotal: number, notesText: string): NotesLayout {
	const collapsible = subListMode(subListTotal) === "tasks";
	return { collapsible, open: !collapsible || notesText.trim() !== "" };
}

/** What the notes body renders. `unavailable` is the case this task fixes: the daemon answered
 * `GetNotes` with no path for a task whose line carries a `ref:` tag, so it resolved the folder
 * somewhere else than this client (an older daemon) — an empty editor there would silently write
 * a second notes.md. */
export type NotesMode = "loading" | "no-task-id" | "unavailable" | "editor";

export function notesMode(input: {
	parentLine: string;
	parentTaskId: string;
	hasRefTag: boolean;
	notesPathMissing: boolean;
}): NotesMode {
	if (!input.parentLine) return "loading";
	if (!input.parentTaskId) return "no-task-id";
	if (input.hasRefTag && input.notesPathMissing) return "unavailable";
	return "editor";
}
