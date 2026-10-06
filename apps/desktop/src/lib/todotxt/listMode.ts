// List mode on the Tasks buffer (specs/client-parity.toml list.*; the TUI's keys): while nothing
// has the keyboard, the caret's line is the current line and single keys act on it. This module
// is the pure part: where the current line goes, and the mutations a command sends. FileView
// applies them through the daemon, as every other edit.
import type { Mutation, TaskRef } from "../daemon";

/** The document as the daemon last gave it. */
export interface ListDoc {
	lines: string[];
	/** One per line, "" for none (`FileContents.task_ids`). */
	taskIds: string[];
}

/** The doc's lines without the final line ending's empty "line". */
export function docLines(text: string): string[] {
	const lines = text.split("\n");
	if (lines.at(-1) === "") lines.pop();
	return lines;
}

const isTask = (line: string | undefined) => line !== undefined && line.trim() !== "";

/** Where a movement command takes the current line (1-based), or null for any other command. */
export function moveTarget(id: string, current: number, doc: ListDoc): number | null {
	const count = Math.max(doc.lines.length, 1);
	const lastTask = doc.lines.findLastIndex(isTask) + 1 || 1;
	switch (id) {
		case "list.down":
			return Math.min(current + 1, count);
		case "list.up":
			return Math.max(current - 1, 1);
		case "list.first":
			return 1;
		case "list.last":
			return lastTask;
		default:
			return null;
	}
}

function ref(doc: ListDoc, line: number): TaskRef {
	return { line_number: line, task_id: doc.taskIds[line - 1] ?? "" };
}

/** What a line-changing command sends, and where the current line should be after it lands; null
 * when there is nothing to do (a blank line, the edge of the list). `reopen` turns a done line back
 * into an open one by the shared core's rule (`txtodo_core::chips`, the `x` chip). */
export async function listMutations(
	id: string,
	current: number,
	doc: ListDoc,
	today: string,
	reopen: (line: string) => Promise<string>
): Promise<{ mutations: Mutation[]; cursor: number } | null> {
	const line = doc.lines[current - 1];
	if (!isTask(line)) return null;
	switch (id) {
		case "list.toggle_complete":
			if (/^x /.test(line)) {
				return { mutations: [{ kind: "edit", task: ref(doc, current), new_line: await reopen(line) }], cursor: current };
			}
			return { mutations: [{ kind: "complete", task: ref(doc, current), today }], cursor: current };
		case "list.delete":
			return {
				mutations: [{ kind: "delete", task: ref(doc, current), leave_blank: false }],
				cursor: Math.min(current, Math.max(doc.lines.length - 1, 1))
			};
		case "list.move_down":
			// The line below moves above this one: the same swap, and it works at the list's end.
			if (!isTask(doc.lines[current])) return null;
			return {
				mutations: [{ kind: "move_before", task: ref(doc, current + 1), before: ref(doc, current) }],
				cursor: current + 1
			};
		case "list.move_up":
			if (current < 2 || !isTask(doc.lines[current - 2])) return null;
			return {
				mutations: [{ kind: "move_before", task: ref(doc, current), before: ref(doc, current - 1) }],
				cursor: current - 1
			};
		default:
			return null;
	}
}
