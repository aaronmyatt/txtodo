// CodeMirror 6 decorations for the main view (plan §3.1): hidden `id:` tags, muted + struck
// completed lines, and trailing `ref:` progress/notes indicators. Token *colours* are
// NOT this module's concern — those come from `todotxtLanguage`'s own theme/`styleTags`
// (tasks/desktop-main-view/notes.md: "don't hardcode colours yourself, that's the grammar task's
// concern"). Everything here is viewport-bounded (only `view.visibleRanges`) to hold the
// 10k-line first-paint budget (plan M7 acceptance, tasks/desktop-main-view/todo.txt #8).
//
// CM6 decorations: https://codemirror.net/docs/ref/#view.Decoration
// CM6 MatchDecorator (viewport-bounded regex decorations): https://codemirror.net/docs/ref/#view.MatchDecorator
// CM6 Compartment (reconfigurable extension slot): https://codemirror.net/docs/ref/#state.Compartment
//
// Each `FileView` instance owns its own `Compartment`s (created locally, not shared from here) so
// multiple simultaneous file views — the root and a future detail view's nested ones — never
// reconfigure each other's editor state.
import { RangeSetBuilder, type Extension } from "@codemirror/state";
import {
	Decoration,
	type DecorationSet,
	EditorView,
	MatchDecorator,
	ViewPlugin,
	type ViewUpdate,
	WidgetType
} from "@codemirror/view";
import { completedLineInfo, findRefTag, resolveRefIndicator, type FileProgress, type RefIndicator } from "./lineInfo";

const idTagMatcher = new MatchDecorator({
	regexp: /\bid:\S+/g,
	// Zero-width: `Decoration.replace({})` with no widget hides the matched text entirely.
	decoration: () => Decoration.replace({})
});

/** Hides `id:` tags inline. They stay in the document and in the edit popover — §3.1. */
export const idTagsHidden: Extension = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = idTagMatcher.createDeco(view);
		}
		update(update: ViewUpdate) {
			this.decorations = idTagMatcher.updateDeco(update, this.decorations);
		}
	},
	{ decorations: (v) => v.decorations }
);

/** Ghost text ("Add a line…") on the document's last line, only while that line is empty — a
 * blank trailing line is already a real document line (design §2.6: blanks are entries), and the
 * daemon appends a new task there, so this needs no dedicated "add" affordance of its own: typing
 * over the placeholder and committing (blur/Cmd-S) goes through the exact same delta-commit path
 * as any other edit (rawMode.ts's `computeDelta` already turns a new untagged line into an `Add`). */
class AddLinePlaceholderWidget extends WidgetType {
	eq(): boolean {
		return true;
	}

	toDOM(): HTMLElement {
		const span = document.createElement("span");
		span.className = "cm-todotxt-add-line-placeholder";
		span.textContent = "Add a line…";
		return span;
	}

	ignoreEvent(): boolean {
		return true; // let the click fall through to CM6's own "place the cursor here"
	}
}

function buildAddLinePlaceholder(view: EditorView): DecorationSet {
	const lastLine = view.state.doc.line(view.state.doc.lines);
	if (lastLine.length !== 0) return Decoration.none;
	const builder = new RangeSetBuilder<Decoration>();
	builder.add(lastLine.from, lastLine.from, Decoration.widget({ widget: new AddLinePlaceholderWidget(), side: 1 }));
	return builder.finish();
}

export const addLinePlaceholder: Extension = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = buildAddLinePlaceholder(view);
		}
		update(update: ViewUpdate) {
			if (update.docChanged) this.decorations = buildAddLinePlaceholder(update.view);
		}
	},
	{ decorations: (v) => v.decorations }
);

/** Renders `n/m` (open/total) or a notes icon at the end of a `ref:` line. */
class RefIndicatorWidget extends WidgetType {
	constructor(private readonly indicator: RefIndicator) {
		super();
	}

	eq(other: RefIndicatorWidget): boolean {
		const a = this.indicator;
		const b = other.indicator;
		return a.kind === b.kind && (a.kind !== "progress" || (b.kind === "progress" && a.done === b.done && a.total === b.total));
	}

	toDOM(): HTMLElement {
		const span = document.createElement("span");
		span.className = "cm-todotxt-ref-indicator";
		if (this.indicator.kind === "progress") {
			const { done, total } = this.indicator;
			span.textContent = `${done}/${total}`;
			span.setAttribute("aria-label", `${done} of ${total} done`);
		} else {
			span.textContent = "\u{1F4DD}"; // notes icon (memo)
			span.setAttribute("aria-label", "has notes");
		}
		return span;
	}

	ignoreEvent(): boolean {
		return true;
	}
}

function buildLineDecorations(
	view: EditorView,
	containingPath: string,
	filesByPath: ReadonlyMap<string, FileProgress>
): DecorationSet {
	const builder = new RangeSetBuilder<Decoration>();
	for (const { from, to } of view.visibleRanges) {
		let pos = from;
		while (pos <= to) {
			const line = view.state.doc.lineAt(pos);

			const completed = completedLineInfo(line.text);
			if (completed) {
				builder.add(line.from, line.from, Decoration.line({ class: "cm-todotxt-done" }));
				if (completed.descriptionStart < line.text.length) {
					builder.add(
						line.from + completed.descriptionStart,
						line.to,
						Decoration.mark({ class: "cm-todotxt-strike" })
					);
				}
			}

			const ref = findRefTag(line.text);
			if (ref) {
				const indicator = resolveRefIndicator(containingPath, ref.slug, filesByPath);
				if (indicator) {
					builder.add(line.to, line.to, Decoration.widget({ widget: new RefIndicatorWidget(indicator), side: 1 }));
				}
			}

			pos = line.to + 1;
		}
	}
	return builder.finish();
}

/**
 * Completed-line muting/strike-through and trailing `ref:` indicators. `containingPath` and
 * `filesByPath` are captured at construction time — `FileView` rebuilds this extension (via its
 * own `Compartment`) when either changes, which in practice is only on mount and on a path
 * switch, not on every keystroke/`Change`.
 */
export function lineDecorations(containingPath: string, filesByPath: ReadonlyMap<string, FileProgress>): Extension {
	return ViewPlugin.fromClass(
		class {
			decorations: DecorationSet;
			constructor(view: EditorView) {
				this.decorations = buildLineDecorations(view, containingPath, filesByPath);
			}
			update(update: ViewUpdate) {
				if (update.docChanged || update.viewportChanged) {
					this.decorations = buildLineDecorations(update.view, containingPath, filesByPath);
				}
			}
		},
		{ decorations: (v) => v.decorations }
	);
}

/** Structural (non-token) styling this view owns directly; token colours live in `todotxtLanguage`. */
export const mainViewBaseTheme = EditorView.baseTheme({
	".cm-todotxt-done": { opacity: "0.55" },
	".cm-todotxt-strike": { textDecoration: "line-through" },
	".cm-todotxt-hover": { backgroundColor: "rgba(15, 23, 42, 0.05)" },
	".cm-todotxt-add-line-placeholder": { color: "#9ca3af", fontStyle: "italic", pointerEvents: "none" },
	".cm-todotxt-ref-indicator": {
		marginLeft: "0.5em",
		fontSize: "0.85em",
		opacity: "0.7",
		userSelect: "none"
	}
});
