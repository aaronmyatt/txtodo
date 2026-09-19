// The line-length hint's one measure for the desktop editor (root todos ids
// 01M2WK5DQQ2H17Z53VJGNB0JHP, 01M2WK5DQQ7RBVT97E0ABZYZQK, 01M2WK5DQRW482BR9A7VXCG50M). The
// authority is `crates/txtodo-core/src/line_length.rs`; this mirrors it, and `lineLength.test.ts`
// carries the same vectors — change one, change both.
//
// The measure is what a human sees: Unicode scalar values (a code point, so an emoji is 1, not the
// 2 UTF-16 units `String.length` gives it), without the line's own `id:` tag and the blanks in
// front of it — the editor hides the tag (`idTagsHidden`), and a 30-char tag on a 75-char line
// must not flag it. Advisory only: nothing here stops a longer line being typed or saved. One known
// difference from the Rust measure: the editor hides any `id:<word>` (its `idTagsHidden` matcher),
// core only a well-formed `id:<ULID>`, so a malformed tag is invisible here but counted there.
import { findIdTagRanges, type IdTagRange } from "./lineInfo";

/** root todo 9: "add line length hints to the clients... to encourage keeping todo entries
 * readable". 100 matches the line-width budget this project's own Rust code is held to
 * (`.claude/budgets.json`'s `lineWidth`), not a todo.txt-format rule. */
export const LINE_LENGTH_HINT = 100;

/** Each hidden `id:` tag widened left over the spaces/tabs before it, so the blank the tag was
 * separated by is not counted either. Ascending and non-overlapping. */
function hiddenRanges(text: string): IdTagRange[] {
	return findIdTagRanges(text).map(({ from, to }) => {
		let start = from;
		while (start > 0 && (text[start - 1] === " " || text[start - 1] === "\t")) start--;
		return { from: start, to };
	});
}

/** Calls `visit(codePointCount, utf16Index)` for each visible character, stopping if it returns true. */
function walkVisible(text: string, visit: (seen: number, index: number) => boolean): void {
	const hidden = hiddenRanges(text);
	let h = 0;
	let seen = 0;
	let index = 0;
	for (const ch of text) {
		while (h < hidden.length && index >= hidden[h].to) h++;
		const isHidden = h < hidden.length && index >= hidden[h].from;
		if (!isHidden) {
			if (visit(seen, index)) return;
			seen++;
		}
		index += ch.length;
	}
}

/** The visible length of `text`, in code points. */
export function visibleLength(text: string): number {
	let count = 0;
	walkVisible(text, (seen) => {
		count = seen + 1;
		return false;
	});
	return count;
}

/** The UTF-16 index in `text` of the first character past the hint, or `null` when the line is
 * within it — where the editor starts its underline. */
export function hintOffset(text: string): number | null {
	let offset: number | null = null;
	walkVisible(text, (seen, index) => {
		if (seen < LINE_LENGTH_HINT) return false;
		offset = index;
		return true;
	});
	return offset;
}
