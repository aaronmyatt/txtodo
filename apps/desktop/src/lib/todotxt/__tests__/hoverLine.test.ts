// The hovered-line highlight must stay inside the document across any change. EditorState only, no
// DOM: the view's own step that throws is "map the start state's decorations through the
// transaction's changes", so `mapsCleanly` runs that same step directly.
// RangeSet.map: https://codemirror.net/docs/ref/#state.RangeSet.map
import { describe, expect, it } from "vitest";
import { EditorState, RangeSetBuilder, type Extension, type Transaction } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import { hoverLine, setHoverLine } from "../hoverLine";

function stateWith(doc: string, extension: Extension = hoverLine): EditorState {
	return EditorState.create({ doc, extensions: [extension] });
}

/** Every decoration set in the state. The `EditorView.decorations` facet also takes functions of
 * a view (ViewPlugins); none are installed here, so each value is a set.
 * Ref: https://codemirror.net/docs/ref/#view.EditorView^decorations */
function decorationSets(state: EditorState): DecorationSet[] {
	return state.facet(EditorView.decorations).filter((d): d is DecorationSet => typeof d !== "function");
}

/** `{ from, class }` for each decoration, read with a cursor so an out-of-range one still shows.
 * Ref: https://codemirror.net/docs/ref/#state.RangeSet.iter */
function hovered(state: EditorState): { from: number; cls: string }[] {
	const out: { from: number; cls: string }[] = [];
	for (const set of decorationSets(state)) {
		for (const cursor = set.iter(); cursor.value; cursor.next()) {
			out.push({ from: cursor.from, cls: cursor.value.spec.class });
		}
	}
	return out;
}

function hover(state: EditorState, lineNumber: number | null): EditorState {
	return state.update({ effects: setHoverLine.of(lineNumber) }).state;
}

/** What `FileView.refreshDoc` dispatches for a synced edit: one change over the whole file. */
function replaceAll(state: EditorState, text: string): Transaction {
	return state.update({ changes: { from: 0, to: state.doc.length, insert: text } });
}

function mapsCleanly(tr: Transaction): void {
	for (const set of decorationSets(tr.startState)) set.map(tr.changes);
}

function lines(count: number): string {
	return Array.from({ length: count }, (_, i) => `task ${i + 1}`).join("\n");
}

describe("hoverLine", () => {
	it("marks the start of the hovered line", () => {
		const state = hover(stateWith("a\nbb\nccc"), 2);
		expect(state.field(hoverLine)).toBe(2);
		expect(hovered(state)).toEqual([{ from: 2, cls: "cm-todotxt-hover" }]);
	});

	it("clears with null", () => {
		const state = hover(hover(stateWith("a\nbb\nccc"), 2), null);
		expect(state.field(hoverLine)).toBeNull();
		expect(hovered(state)).toEqual([]);
	});

	it("shows nothing for a line past the end", () => {
		expect(hovered(hover(stateWith("a\nbb"), 3))).toEqual([]);
	});

	it("moves with its line when text is typed above it", () => {
		const state = hover(stateWith("a\nbb\nccc"), 3);
		const typed = state.update({ changes: { from: 0, insert: "xx" } }).state;
		expect(hovered(typed)).toEqual([{ from: 7, cls: "cm-todotxt-hover" }]);
	});

	it("stays in range across two synced edits that shrink the file under a still mouse", () => {
		const start = hover(stateWith(lines(200)), 200);

		const shrink = replaceAll(start, lines(100));
		expect(() => mapsCleanly(shrink)).not.toThrow();
		for (const { from } of hovered(shrink.state)) expect(from).toBeLessThanOrEqual(shrink.state.doc.length);

		// The second change is the one that threw: it maps the first one's leftover highlight.
		const next = replaceAll(shrink.state, lines(150));
		expect(() => mapsCleanly(next)).not.toThrow();
	});

	it("control: the old fixed decoration set throws in the same sequence", () => {
		// The old `hoverDecorationFor`: a set built once against the document of that moment.
		const long = lines(200);
		const lastLineFrom = long.lastIndexOf("\n") + 1;
		const builder = new RangeSetBuilder<Decoration>();
		builder.add(lastLineFrom, lastLineFrom, Decoration.line({ class: "cm-todotxt-hover" }));
		const start = stateWith(long, EditorView.decorations.of(builder.finish()));

		const shrink = replaceAll(start, lines(100));
		expect(() => mapsCleanly(shrink)).not.toThrow();
		const next = replaceAll(shrink.state, lines(150));
		expect(() => mapsCleanly(next)).toThrow(/out of range for changeset/);
	});
});
