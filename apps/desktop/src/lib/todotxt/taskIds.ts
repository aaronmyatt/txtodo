// Which task a line is (task sidecar-task-ids). Pure: no DOM, no Tauri — same split as
// `./rawMode.ts`.
//
// Under Sidecar identity (ADR 0019) a todo.txt line carries no `id:` tag, so the text cannot say
// which task it is. The daemon sends one id per line with `GetFile` (`FileContents.task_ids`);
// that list wins. The `id:` tag in the text is only the fallback, for a daemon older than the
// field or a Tagged workspace read through one.

/** The part of `FileContents` this module needs (kept structural so tests need no daemon types). */
export interface LinesWithIds {
	text: string;
	task_ids?: string[];
}

const ID_TAG = /\bid:(\S+)/;

/**
 * The task id of 1-based `lineNumber`, or `""` when the line has none (a blank line, or a line
 * past the end). `""` is the same "no id" value `TaskRef.task_id` already uses.
 *
 * String.prototype.split: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String/split
 */
export function taskIdAt(contents: LinesWithIds, lineNumber: number): string {
	const fromDaemon = contents.task_ids?.[lineNumber - 1];
	if (fromDaemon) return fromDaemon;
	// A daemon that sent ids for this document has the last word: an empty entry means "blank
	// line", and a leftover `id:` word in a Sidecar line's text is plain text, not an identity.
	if (contents.task_ids && contents.task_ids.length > 0) return "";
	const line = contents.text.split("\n")[lineNumber - 1] ?? "";
	const match = ID_TAG.exec(line);
	return match ? match[1] : "";
}

/**
 * The 1-based line `taskId` sits on now, or `fallbackLine` when the daemon sent no ids or the task
 * is gone. A view pinned to a line number goes stale the moment that line moves, and completing a
 * task moves it to the bottom of its file (task complete-to-bottom); the id is what stays true.
 *
 * Array.prototype.indexOf: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/indexOf
 */
export function lineOfTask(contents: LinesWithIds, taskId: string, fallbackLine: number): number {
	if (!taskId || !contents.task_ids) return fallbackLine;
	const index = contents.task_ids.indexOf(taskId);
	return index === -1 ? fallbackLine : index + 1;
}
