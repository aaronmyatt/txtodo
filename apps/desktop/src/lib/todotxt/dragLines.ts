// Drag a line to reorder it (specs/client-parity.toml list.move_up/list.move_down: "or drag the
// row", which only the TUI did): press on a line's number in the gutter, drag, and let go above
// the line it should go before. The move is an ordinary buffer edit that only reorders lines, so
// FileView's reorder save writes it after a short pause, exactly as Alt-Up/Down does.
// CM6 gutters and decorations: https://codemirror.net/docs/ref/#view.lineNumbers,
// https://codemirror.net/docs/ref/#view.Decoration^line
import { StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

/** `text` with line `from` (1-based) moved to sit before line `before` (`lines + 1` = the end),
 * the final line ending kept as it was. Unchanged when the move is a no-op. */
export function moveLineText(text: string, from: number, before: number): string {
	const ending = text.endsWith("\n");
	const lines = (ending ? text.slice(0, -1) : text).split("\n");
	if (from < 1 || from > lines.length || before === from || before === from + 1) return text;
	const [line] = lines.splice(from - 1, 1);
	const at = before > from ? before - 2 : before - 1;
	lines.splice(Math.max(0, Math.min(at, lines.length)), 0, line);
	return lines.join("\n") + (ending ? "\n" : "");
}

/** The line a drop at `y` (client px) goes before: the hovered line, or the next one when the
 * pointer is on its lower half; past the last line, the end. */
export function dropBefore(view: EditorView, y: number): number {
	const doc = view.state.doc;
	const lastTextLine = doc.line(doc.lines).length === 0 && doc.lines > 1 ? doc.lines - 1 : doc.lines;
	const block = view.lineBlockAtHeight(y - view.documentTop);
	const n = doc.lineAt(block.from).number;
	if (n > lastTextLine) return lastTextLine + 1;
	return y > view.documentTop + block.top + block.height / 2 ? n + 1 : n;
}

const setDropLine = StateEffect.define<number | null>();
const dropMark = Decoration.line({ class: "cm-drop-before" });
const dropEnd = Decoration.line({ class: "cm-drop-after" });

const dropField = StateField.define<DecorationSet>({
	create: () => Decoration.none,
	update(deco, tr) {
		for (const e of tr.effects) {
			if (!e.is(setDropLine)) continue;
			if (e.value === null) return Decoration.none;
			const doc = tr.state.doc;
			if (e.value > doc.lines) return Decoration.set([dropEnd.range(doc.line(doc.lines).from)]);
			return Decoration.set([dropMark.range(doc.line(e.value).from)]);
		}
		return deco.map(tr.changes);
	},
	provide: (f) => EditorView.decorations.from(f)
});

const dragTheme = EditorView.baseTheme({
	".cm-drop-before": { boxShadow: "inset 0 2px 0 var(--color-base-content)" },
	".cm-drop-after": { boxShadow: "inset 0 -2px 0 var(--color-base-content)" },
	".cm-lineNumbers .cm-gutterElement": { cursor: "grab", userSelect: "none" }
});

/** The drop marker's state and look; FileView adds it beside the gutter. */
export function dragLines(): Extension {
	return [dropField, dragTheme];
}

/** Starts dragging line `from` from a gutter press; resolves on release. Returns true when it
 * handled the press (so the gutter's own selection does not run). */
export function startLineDrag(view: EditorView, from: number, event: MouseEvent): boolean {
	if (event.button !== 0 || view.state.readOnly) return false;
	let before: number | null = null;
	const move = (e: MouseEvent) => {
		before = dropBefore(view, e.clientY);
		const shown = before === from || before === from + 1 ? null : before;
		view.dispatch({ effects: setDropLine.of(shown) });
	};
	const up = () => {
		window.removeEventListener("mousemove", move);
		window.removeEventListener("mouseup", up);
		view.dispatch({ effects: setDropLine.of(null) });
		if (before === null) return;
		const text = view.state.doc.toString();
		const next = moveLineText(text, from, before);
		if (next === text) return;
		const landed = before > from ? before - 1 : before;
		view.dispatch({ changes: { from: 0, to: text.length, insert: next } });
		view.dispatch({ selection: { anchor: view.state.doc.line(Math.min(landed, view.state.doc.lines)).from } });
	};
	// mousemove / mouseup on the window, so a drag that leaves the editor still ends:
	// https://developer.mozilla.org/en-US/docs/Web/API/Element/mousemove_event
	window.addEventListener("mousemove", move);
	window.addEventListener("mouseup", up);
	event.preventDefault();
	return true;
}
