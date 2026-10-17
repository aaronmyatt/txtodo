// Move or copy a task to another list (task add-the-ability-to-move-or-copy-a-task-t): which lists
// a line can go to, and the text a copy adds. A move is the daemon's cross-file `Move`, which keeps
// the task's id and takes its `ref:` folder along; a copy is an `Add` there, a new task.
import type { FileInfo } from "../daemon";

const depth = (path: string) => path.split("/").length;

/** The workspace's lists other than `from`, as the daemon names them: the root list first, then
 * by depth and name. A notes file is not a list (`FileInfo.kind` is `FILE_KIND_NOTES` from the
 * daemon, `notes` from the mock). */
export function otherLists(files: Iterable<FileInfo>, from: string): string[] {
	const isList = (f: FileInfo) => f.path.endsWith(".txt") && !/notes/i.test(f.kind);
	const paths = [...files].filter(isList).map((f) => f.path);
	return paths.filter((p) => p !== from).sort((a, b) => depth(a) - depth(b) || a.localeCompare(b));
}

/** A copy is a new task: the line without its `id:` (the daemon mints one) and its `ref:` (the
 * notes and sub-list stay with the original). */
export function copyText(line: string): string {
	return line
		.split(" ")
		.filter((word) => !/^(id|ref):\S/.test(word))
		.join(" ");
}
