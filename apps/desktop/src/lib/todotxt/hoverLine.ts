// The main view's hovered-line highlight. The line number lives in editor state and the
// decoration is rebuilt from the current document, so a text change (typing, or a synced edit
// from another device repainting the whole file) can never leave it pointing past the end.
// It used to be a fixed `EditorView.decorations.of(set)` in a compartment: CM6 never remaps a
// fixed set, so after the document shrank below it the next change threw "Position N is out of
// range for changeset of length M".
//
// CM6 decorations from state fields: https://codemirror.net/examples/decoration/
import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView } from "@codemirror/view";

/** Moves the highlight to a 1-based line number, or clears it with `null`.
 * Ref: https://codemirror.net/docs/ref/#state.StateEffect^define */
export const setHoverLine = StateEffect.define<number | null>();

/** The hovered line number (1-based), or `null`. Read it with `state.field(hoverLine)`.
 * Ref: https://codemirror.net/docs/ref/#state.StateField */
export const hoverLine = StateField.define<number | null>({
	create: () => null,
	update(value, tr) {
		for (const effect of tr.effects) if (effect.is(setHoverLine)) return effect.value;
		return value;
	},
	// `"doc"` as a dependency recomputes the decoration on every document change, not only when
	// the line number changes, so its position is always inside the current document. A line
	// number past the end (the file shrank under a still mouse) just shows nothing until the next
	// mousemove. Ref: https://codemirror.net/docs/ref/#state.Facet.compute
	provide: (field) =>
		EditorView.decorations.compute(["doc", field], (state) => {
			const lineNumber = state.field(field);
			if (lineNumber == null || lineNumber < 1 || lineNumber > state.doc.lines) return Decoration.none;
			const line = state.doc.line(lineNumber);
			return Decoration.set(Decoration.line({ class: "cm-todotxt-hover" }).range(line.from));
		})
});
