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
import { hintOffset } from "./lineLength";
import { get } from "svelte/store";
import { workspaceLayoutStore } from "$lib/stores/workspaces";

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

const refTagMatcher = new MatchDecorator({
	regexp: /\bref:\S+/g,
	decoration: () => Decoration.replace({})
});

/** Hides `ref:` tags inline, for the detail view's parent line (root todo "ref tag path is visible
 * in the desktop detail view"); `keepRefTag` puts the original back on save. Atomic, so Backspace
 * next to one cannot eat it a character at a time.
 * https://codemirror.net/docs/ref/#view.EditorView^atomicRanges */
export const refTagsHidden: Extension = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = refTagMatcher.createDeco(view);
		}
		update(update: ViewUpdate) {
			this.decorations = refTagMatcher.updateDeco(update, this.decorations);
		}
	},
	{
		decorations: (v) => v.decorations,
		provide: (plugin) => EditorView.atomicRanges.of((view) => view.plugin(plugin)?.decorations ?? Decoration.none)
	}
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
				const indicator = resolveRefIndicator(containingPath, ref.slug, filesByPath, get(workspaceLayoutStore));
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

/** root todo 9: "add line length hints to the clients... to encourage keeping todo entries
 * readable" — a plain todo.txt line has no wrap, so a very long one is easy to write without
 * noticing. The measure (visible characters, the hidden `id:` tag not counted) and the 100 limit
 * live in `./lineLength`; purely advisory, nothing here stops a longer line from being typed or
 * saved. */
function buildLongLineHint(view: EditorView): DecorationSet {
	const builder = new RangeSetBuilder<Decoration>();
	for (const { from, to } of view.visibleRanges) {
		let pos = from;
		while (pos <= to) {
			const line = view.state.doc.lineAt(pos);
			const past = hintOffset(line.text);
			if (past !== null) {
				builder.add(line.from + past, line.to, Decoration.mark({ class: "cm-todotxt-long-line" }));
			}
			pos = line.to + 1;
		}
	}
	return builder.finish();
}

/** Marks the part of a line past the hint with a subtle underline — see `buildLongLineHint`'s
 * own doc. Viewport-bounded and re-built on doc/viewport change, same shape as `lineDecorations`. */
export const longLineHint: Extension = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = buildLongLineHint(view);
		}
		update(update: ViewUpdate) {
			if (update.docChanged || update.viewportChanged) {
				this.decorations = buildLongLineHint(update.view);
			}
		}
	},
	{ decorations: (v) => v.decorations }
);

/** Structural (non-token) styling this view owns directly; token colours live in `todotxtLanguage`.
 * CM6's own default theme hardcodes the editor background/caret to white/black
 * (https://github.com/codemirror/view/blob/main/src/theme.ts), so `&` (the `.cm-editor` root —
 * https://codemirror.net/docs/ref/#view.EditorView^baseTheme) and `.cm-cursor` here override those
 * with app.css's tokens; everything else in this theme was already color-scheme-agnostic. */
export const mainViewBaseTheme = EditorView.baseTheme({
	// The buffer's chrome (task desktop-ui-revamp, the c2 mockup's editor): text size and a px line
	// height from CSS vars (editorPrefs sets them; px because decoration row maths assume it), a
	// line-number gutter behind a 10% ink rule, the caret's line tinted 7%, and a block caret.
	"&": {
		backgroundColor: "var(--color-bg)",
		color: "var(--color-text)",
		fontSize: "var(--editor-font-size, 15px)"
	},
	".cm-scroller": { lineHeight: "var(--editor-line-height, 28px)", fontVariantLigatures: "none" },
	".cm-content": { padding: "1rem 0" },
	".cm-line": { padding: "0 1.25rem" },
	".cm-gutters": {
		backgroundColor: "transparent",
		borderRight: "2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent)",
		color: "color-mix(in srgb, var(--color-base-content) 40%, transparent)"
	},
	".cm-lineNumbers .cm-gutterElement": { padding: "0 0.75rem 0 1rem", fontSize: "0.75em" },
	".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-base-content) 7%, transparent)" },
	".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--color-base-content)" },
	// A block over the character, see-through so the letter under it still reads: ink at 35%.
	// (A difference blend does not work here: CM's cursor layer sits in its own stacking context,
	// so it blends with nothing and paints solid.) CSS, not `caret-shape`, which is Chromium-only
	// and WKWebView ignores. https://codemirror.net/docs/ref/#view.drawSelection
	".cm-cursor, .cm-dropCursor": {
		borderLeft: "none",
		width: "0.6em",
		marginLeft: "0",
		backgroundColor: "color-mix(in srgb, var(--color-base-content) 35%, transparent)"
	},
	".cm-todotxt-done": { opacity: "0.55" },
	".cm-todotxt-strike": { textDecoration: "line-through" },
	".cm-todotxt-hover": { backgroundColor: "var(--color-hover-overlay)" },
	".cm-todotxt-ref-indicator": {
		marginLeft: "0.5em",
		fontSize: "0.85em",
		opacity: "0.7",
		userSelect: "none"
	},
	".cm-todotxt-long-line": {
		textDecoration: "underline wavy var(--color-text-muted)",
		textDecorationSkipInk: "none"
	}
});
