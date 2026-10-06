// The "Add a line…" input above a list (root todo line "the Add a line input should be at the top
// of the page"; TUI parity id `list.add_on_top`): the line is sent as an `add`, which the daemon
// appends right after the last task, then a `move_before` the first task in the same `Apply`, so it
// lands at the top. The TUI builds the same pair (`crates/txtodo-tui/src/ui/edit.rs::commit_all`).
import type { Mutation, TaskRef } from "$lib/daemon";

/** The mutations that add `line` at the top of the document whose text is `text` (the daemon's
 * last-read baseline) and whose task ids are `taskIds` (one per line, "" for none). An empty
 * document gets only the `add`. */
export function addOnTopMutations(text: string, taskIds: string[], line: string): Mutation[] {
	const add: Mutation = { kind: "add", line };
	const lines = text.split("\n");
	if (lines.at(-1) === "") lines.pop(); // the final line ending is not a line
	const isTask = (l: string) => l.trim() !== "";
	const first = lines.findIndex(isTask);
	const last = lines.findLastIndex(isTask);
	if (first < 0) return [add];
	// Appended one line below the last task, 1-based; addressed by number (it has no id yet).
	const task: TaskRef = { line_number: last + 2, task_id: "" };
	const before: TaskRef = { line_number: first + 1, task_id: taskIds[first] ?? "" };
	return [add, { kind: "move_before", task, before }];
}
