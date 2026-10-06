// The sub-toolbar's counts (task desktop-ui-revamp, plan §3.2.5): open tasks (`t`), done tasks
// (`x`), and the document's lines. Blank lines are not tasks, so they count in neither `t` nor `x`;
// they do count as lines. Display arithmetic on text, like lineInfo.ts: never a task data model.

export interface DocCounts {
	open: number;
	done: number;
	lines: number;
}

/** Counts `text` (a whole document; its final line ending ends the last line, not a new one). */
export function docCounts(text: string): DocCounts {
	const lines = text === "" ? [] : text.replace(/\n$/, "").split("\n");
	let open = 0;
	let done = 0;
	for (const line of lines) {
		if (line.trim() === "") continue;
		if (line.startsWith("x ")) done++;
		else open++;
	}
	return { open, done, lines: lines.length };
}
