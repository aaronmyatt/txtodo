// The mark as a progress meter (task desktop-ui-revamp, "Live mark"): what the prompt bar's mark
// shows for the root list. The counts are the daemon's (`ListFiles`' `done`/`total`, blanks
// excluded per plan §3.2.5), never a client count.
import type { FileInfo } from "../daemon";

export interface Meter {
	/** 0..1 for `Mark`'s `progress`; 0 with no tasks, so nothing is drawn. */
	progress: number;
	/** The tooltip. */
	title: string;
}

/** The meter for `path` among `files`; an unknown path reads as no tasks. */
export function meterFor(files: FileInfo[], path: string): Meter {
	const file = files.find((f) => f.path === path);
	const total = file?.total ?? 0;
	const done = Math.min(file?.done ?? 0, total);
	if (total === 0) return { progress: 0, title: "No tasks yet" };
	return { progress: done / total, title: `${done} of ${total} done` };
}
