// Duplicate lines (ADR 0032, task sync-drift duplicate-flags): two or more lines of one file that
// read the same. Pure helpers, no Tauri imports, so they are unit-tested without mocking `invoke`
// (duplicates.test.ts). The daemon derives the groups (`listDuplicates`); a group is resolved by an
// ordinary `delete` of the other copies, or by editing one so they differ. Unlike a `needs_review`
// flag it never makes the buffer read-only.
import type { DuplicateGroup, Mutation } from "$lib/daemon";

/** Which copy survives. Newest is the safe default (ADR 0032: on a device that re-minted a file,
 * deleting the older id deletes nothing there). */
export type KeepChoice = "newest" | "oldest";

/** The deletes that leave only the copy `keep` names. By task id alone (`line_number: 0`): the
 * daemon resolves a line number first, and the second delete's line has moved by then (under
 * Sidecar it would delete whatever is there now; `txtodo-daemon/src/mutation.rs::resolve`). No
 * blank is left behind, as `txtodo conflicts keep-newest`. */
export function keepMutations(group: DuplicateGroup, keep: KeepChoice): Mutation[] {
	if (group.tasks.length < 2) return [];
	const kept = keep === "newest" ? group.tasks.length - 1 : 0;
	return group.tasks
		.filter((_, i) => i !== kept)
		.map((t) => ({
			kind: "delete",
			task: { line_number: 0, task_id: t.task_id },
			leave_blank: false
		}));
}

/** "lines 2 (oldest), 5 and 9 (newest)": where a group's copies are. */
export function copiesLabel(group: DuplicateGroup): string {
	const n = group.tasks.map((t) => t.line_number);
	if (n.length === 0) return "";
	if (n.length === 1) return `line ${n[0]}`;
	const middle = n.slice(1, -1);
	const parts = [`${n[0]} (oldest)`, ...middle.map(String), `${n[n.length - 1]} (newest)`];
	const head = parts.slice(0, -1).join(", ");
	return `lines ${head} and ${parts[parts.length - 1]}`;
}

/** The banner's words for `count` groups. */
export function groupsLabel(count: number): string {
	return count === 1
		? "1 line is in the file twice"
		: `${count} lines are in the file more than once`;
}
