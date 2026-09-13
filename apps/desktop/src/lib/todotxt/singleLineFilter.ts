// Shared CM6 extension: keeps an editor to a single line, stripping any "\n" a paste/IME could
// otherwise introduce. Used by every single-line todo.txt editor (`EditPopover.svelte`,
// `DetailView.svelte`'s pinned parent line) so a todo.txt line — which never embeds a literal
// newline — can't accidentally become two.
// Ref: https://codemirror.net/docs/ref/#state.EditorState^transactionFilter
import { EditorState, type Extension } from "@codemirror/state";

export function singleLineFilter(): Extension {
	return EditorState.transactionFilter.of((tr) => {
		if (!tr.docChanged) return tr;
		const text = tr.newDoc.toString();
		if (!text.includes("\n")) return tr;
		return [
			{
				changes: { from: 0, to: tr.startState.doc.length, insert: text.replace(/\n/g, "") },
				selection: tr.selection
			}
		];
	});
}
