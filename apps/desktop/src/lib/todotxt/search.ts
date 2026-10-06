// Search in a FileView (task desktop-ui-revamp): the query lives in a StateField, set by an effect
// from the header search. Lines that match keep their place and get their hit ranges marked as the
// inverse tile in miniature; lines that don't fade to 30%; nothing is hidden or reordered. Only the
// visible lines are decorated (like decorations.ts), and the full hit list is computed on demand
// (`hitLines`), so a 10k-line buffer never pays for search while typing.
// CM6: https://codemirror.net/docs/ref/#state.StateField, https://codemirror.net/docs/ref/#view.ViewPlugin
import { RangeSetBuilder, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { hitRanges, isActiveQuery, matchesQuery } from "./query";

/** Sets the query (from the header search). */
export const setQuery = StateEffect.define<string>();
/** Sets the current hit's line number (Enter / Shift+Enter), or null. */
export const setCurrentHit = StateEffect.define<number | null>();

const queryField = StateField.define<string>({
	create: () => "",
	update(value, tr) {
		for (const e of tr.effects) if (e.is(setQuery)) return e.value;
		return value;
	}
});

const currentField = StateField.define<number | null>({
	create: () => null,
	update(value, tr) {
		for (const e of tr.effects) {
			if (e.is(setCurrentHit)) return e.value;
			if (e.is(setQuery)) return null;
		}
		return value;
	}
});

const hitMark = Decoration.mark({ class: "cm-hit" });
const dimLine = Decoration.line({ class: "cm-dim" });
const currentLine = Decoration.line({ class: "cm-cur" });

function build(view: EditorView): DecorationSet {
	const query = view.state.field(queryField);
	const builder = new RangeSetBuilder<Decoration>();
	if (!isActiveQuery(query)) return builder.finish();
	const current = view.state.field(currentField);
	for (const { from, to } of view.visibleRanges) {
		for (let pos = from; pos <= to; ) {
			const line = view.state.doc.lineAt(pos);
			if (line.text.trim() !== "") {
				if (matchesQuery(line.text, query)) {
					if (line.number === current) builder.add(line.from, line.from, currentLine);
					for (const [a, b] of hitRanges(line.text, query)) {
						builder.add(line.from + a, line.from + b, hitMark);
					}
				} else {
					builder.add(line.from, line.from, dimLine);
				}
			}
			pos = line.to + 1;
		}
	}
	return builder.finish();
}

const searchDecorations = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = build(view);
		}
		update(u: ViewUpdate) {
			const changed = u.transactions.some((tr) => tr.effects.some((e) => e.is(setQuery) || e.is(setCurrentHit)));
			if (u.docChanged || u.viewportChanged || changed) this.decorations = build(u.view);
		}
	},
	{ decorations: (v) => v.decorations }
);

const searchTheme = EditorView.baseTheme({
	// The inverse tile in miniature: ink block, paper text (token colours included).
	".cm-hit, .cm-hit *": {
		backgroundColor: "var(--color-base-content)",
		color: "var(--color-base-100) !important",
		borderRadius: "0.2em"
	},
	// Two classes, so a miss fades to 30% even on a done line (whose own class sets 55%).
	".cm-line.cm-dim": { opacity: "0.3" },
	".cm-cur": { backgroundColor: "color-mix(in srgb, var(--color-base-content) 10%, transparent)" }
});

/** The search extension for a FileView. */
export function search(): Extension {
	return [queryField, currentField, searchDecorations, searchTheme];
}

/** Every matching line's number, in order (empty for no query). Walks the whole document: call
 * it when the count or a step is wanted, not on every keystroke. */
export function hitLines(state: EditorState): number[] {
	const query = state.field(queryField, false) ?? "";
	if (!isActiveQuery(query)) return [];
	const out: number[] = [];
	let n = 1;
	for (const iter = state.doc.iterLines(); !iter.next().done; n++) {
		const text = iter.value;
		if (text.trim() !== "" && matchesQuery(text, query)) out.push(n);
	}
	return out;
}

/** Moves the current hit to the next (`dir` 1) or previous (-1) matching line, wrapping, and
 * scrolls it to the middle without taking focus. Returns the new current line, its place among
 * the hits (1-based) and the hit count; null when nothing matches. */
export function stepHit(view: EditorView, dir: 1 | -1): { line: number; index: number; total: number } | null {
	const hits = hitLines(view.state);
	if (hits.length === 0) return null;
	const from = view.state.field(currentField) ?? view.state.doc.lineAt(view.state.selection.main.head).number;
	const started = view.state.field(currentField) !== null;
	let i: number;
	if (dir === 1) {
		i = hits.findIndex((n) => (started ? n > from : n >= from));
		if (i === -1) i = 0;
	} else {
		i = hits.findLastIndex((n) => n < from);
		if (i === -1) i = hits.length - 1;
	}
	const line = hits[i];
	view.dispatch({
		effects: [setCurrentHit.of(line), EditorView.scrollIntoView(view.state.doc.line(line).from, { y: "center" })]
	});
	return { line, index: i + 1, total: hits.length };
}
