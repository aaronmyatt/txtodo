// A line as coloured segments outside an editor (task desktop-ui-revamp: Universal rows), from the
// same Lezer grammar and `tok-*` classes the editor uses, so a row reads like the buffer.
import { highlightCode } from "@lezer/highlight";
import { todotxtHighlightStyle, todotxtLanguageObj } from "../lang/todotxtLanguage";

export interface Segment {
	text: string;
	/** `tok-project`, `tok-context`, ... ; "" for plain text. */
	cls: string;
	/** Inside a search hit (`query.ts::hitRanges`). */
	hit?: boolean;
}

/** `segs` split again at the `[from, to)` character `ranges`, the parts inside marked `hit`. */
export function markHits(segs: Segment[], ranges: [number, number][]): Segment[] {
	if (ranges.length === 0) return segs;
	const out: Segment[] = [];
	let pos = 0;
	for (const seg of segs) {
		const end = pos + seg.text.length;
		let at = pos;
		for (const [from, to] of ranges) {
			if (to <= at || from >= end) continue;
			if (from > at) out.push({ text: seg.text.slice(at - pos, from - pos), cls: seg.cls });
			const stop = Math.min(to, end);
			out.push({ text: seg.text.slice(Math.max(from, at) - pos, stop - pos), cls: seg.cls, hit: true });
			at = stop;
		}
		if (at < end) out.push({ text: seg.text.slice(at - pos), cls: seg.cls });
		pos = end;
	}
	return out;
}

/** `text` split into runs by token class. highlightCode walks the parse tree once:
 * https://lezer.codemirror.net/docs/ref/#highlight.highlightCode */
export function segments(text: string): Segment[] {
	const out: Segment[] = [];
	const tree = todotxtLanguageObj.parser.parse(text);
	highlightCode(
		text,
		tree,
		todotxtHighlightStyle,
		(run, cls) => {
			const last = out[out.length - 1];
			if (last && last.cls === cls) last.text += run;
			else out.push({ text: run, cls });
		},
		() => out.push({ text: "\n", cls: "" })
	);
	return out;
}
