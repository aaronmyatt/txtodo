// A line as coloured segments outside an editor (task desktop-ui-revamp: Universal rows), from the
// same Lezer grammar and `tok-*` classes the editor uses, so a row reads like the buffer.
import { highlightCode } from "@lezer/highlight";
import { todotxtHighlightStyle, todotxtLanguageObj } from "../lang/todotxtLanguage";

export interface Segment {
	text: string;
	/** `tok-project`, `tok-context`, ... ; "" for plain text. */
	cls: string;
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
