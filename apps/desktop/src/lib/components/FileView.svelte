<script lang="ts">
	// THE reusable file-rendering component (tasks/desktop-main-view). Binds to `watch([path])`,
	// rebuilds from `get_file(path)` on each matching `Change`, and renders a CM6 `EditorView`
	// (blanks included — design §2.6: blanks are entries) that is directly editable, like a plain
	// text file: click to drop a cursor, type, blur/Cmd-S commits. No popover, no separate "raw
	// mode" — the whole document is always the live buffer; `path`'s own doc comment on `dirty`
	// below covers how an edit gets back to the daemon as an intent-level `Mutation`, never a
	// whole-string replace.
	// `depth` is a hint only (nested indentation); nothing here assumes `depth === 0` — a future
	// detail-view task mounts this again at `depth + 1` for a `ref:` directory's todo.txt.
	//
	// CM6 basics: https://codemirror.net/docs/ref/
	import { onDestroy, onMount, untrack } from "svelte";
	import { Compartment, EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
	import { Decoration, EditorView, keymap } from "@codemirror/view";
	import { defaultKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { applyMutations, getFile, listFiles, onDaemonChange, watchPaths, type FileInfo } from "$lib/daemon";
	import { dirOf } from "$lib/todotxt/lineInfo";
	import { addLinePlaceholder, idTagsHidden, lineDecorations, mainViewBaseTheme } from "$lib/todotxt/decorations";
	import { computeDelta, isNoOpSave } from "$lib/todotxt/rawMode";
	import { flagsForPath, pendingConflicts } from "$lib/stores/conflicts";
	import type { DetailParams } from "$lib/types";

	interface Props {
		path: string;
		depth: number;
		/** Stretches the editor to fill its container's height instead of the default capped
		 * card (see `.editor-shell` below) — the root view (`MainView`) wants the editor to occupy
		 * all remaining space; a nested detail-view sub-list sits among other sections on a normal
		 * scrolling page and keeps the capped look. */
		fill?: boolean;
		/** Optional: fires whenever this instance's buffer becomes dirty/clean, so a host (MainView)
		 * can tell the quick-add hotkey's guard (tasks/desktop-quick-add/notes.md: "if the main
		 * window has an unsaved edit, the hotkey focuses it instead"). */
		onDirtyChange?: (dirty: boolean) => void;
		/** Double-click (or Cmd/Ctrl+Enter) on a line (tasks/desktop-detail-view, plan §3.2): opens
		 * the detail view for that line, whether or not it has a `ref:` tag yet — a task with none
		 * still lazily creates one on the first notes/sub-list edit (plan §3.2.4). Optional so a
		 * `FileView` used somewhere that never wants a detail view (there is none today, but the
		 * component itself shouldn't assume one always exists) doesn't have to pass a no-op. */
		onDetailRequest?: (params: DetailParams) => void;
	}

	let { path, depth, fill = false, onDirtyChange, onDetailRequest }: Props = $props();

	let containerEl: HTMLDivElement | undefined = $state();
	let view: EditorView | undefined;
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let loadError = $state("");
	// Testability hook only (tasks/desktop-visual-regression): the perf test waits on
	// `[data-line-count='10000']` to know the 10k-line fixture has actually reached the editor,
	// rather than guessing a fixed sleep. Kept to a single `$state` + one line in the template —
	// no behavior change for the shipped app.
	let docLineCount = $state(0);

	// The document is always a live, editable buffer (no separate "raw mode" to enter). `dirty`
	// tracks whether it currently differs from `baseline` — the last daemon-confirmed content —
	// and `baseline` is kept up to date by every successful `refreshDoc` while not dirty (never
	// carried across a `path` swap or an unmount unsaved — see the `$effect`/`onDestroy` below,
	// same invariant tasks/desktop-raw-mode's notes.md documented for the mode this replaced).
	let dirty = $state(false);
	let baseline = "";
	// Pending needs_review flags for this instance's own `path` (design §4.7's store, already fed
	// by `ConflictBanner`'s `Watch` subscription): the document being edited is the *reconciled
	// projection*, so a pending flag means it isn't stable yet — read-only until it's resolved.
	const hasPendingReview = $derived(flagsForPath($pendingConflicts, path).length > 0);

	// Per-instance reconfigurable slots (never shared across FileView instances — see
	// $lib/todotxt/decorations.ts) so refreshing ref: progress or the hovered line is a cheap
	// dispatch, not a full document rebuild.
	const lineDecoCompartment = new Compartment();
	const editableCompartment = new Compartment();
	const hoverLineCompartment = new Compartment();

	function setDirty(next: boolean) {
		if (next === dirty) return;
		dirty = next;
		onDirtyChange?.(next);
	}

	function initialExtensions() {
		return [
			editableCompartment.of([EditorView.editable.of(!hasPendingReview), EditorState.readOnly.of(hasPendingReview)]),
			// `Mod-Enter` (Cmd+Enter on macOS, Ctrl+Enter elsewhere — CM6's own convention:
			// https://codemirror.net/docs/ref/#commands) is the keyboard equivalent of a
			// double-click, opening the detail view for the line under the caret (plan §3.2).
			// `Mod-s` commits a dirty buffer early (otherwise blur does it); `Escape` discards it,
			// reverting to `baseline` — everywhere else Escape is unbound today, so this never
			// shadows another command.
			keymap.of([
				{ key: "Mod-Enter", run: openDetailAtSelection },
				{
					key: "Mod-s",
					run: () => {
						if (!dirty) return false;
						commit();
						return true;
					}
				},
				{
					key: "Escape",
					run: () => {
						if (!dirty) return false;
						discard();
						return true;
					}
				},
				...defaultKeymap
			]),
			todotxtLanguage,
			mainViewBaseTheme,
			idTagsHidden, // always hidden — §3.1
			addLinePlaceholder,
			lineDecoCompartment.of(lineDecorations(dirOf(path), filesByPath)),
			hoverLineCompartment.of([]),
			EditorView.domEventHandlers({
				mousemove: handleMouseMove,
				mouseleave: (_event, editorView) => {
					clearHover(editorView);
				},
				dblclick: handleDblClick,
				// Blur commits (same rule tasks/desktop-raw-mode's notes.md stated: "on blur or
				// Cmd/Ctrl+S the buffer goes through the reconciler"). CM6's `blur` domEventHandler
				// fires on the real DOM blur of the content element, i.e. focus actually left the
				// editor — not on the transient focus shuffles a single click within it can cause.
				blur: () => {
					if (dirty) commit();
					return false;
				}
			}),
			EditorView.updateListener.of((u) => {
				if (!u.docChanged) return;
				docLineCount = u.state.doc.lines;
				setDirty(u.state.doc.toString() !== baseline);
			})
		];
	}

	function requestDetail(lineNumber: number) {
		onDetailRequest?.({ file: path, line: lineNumber });
	}

	function openDetailAtSelection(editorView: EditorView): boolean {
		if (!onDetailRequest) return false;
		const line = editorView.state.doc.lineAt(editorView.state.selection.main.head);
		requestDetail(line.number);
		return true;
	}

	function handleDblClick(event: MouseEvent, editorView: EditorView): boolean {
		const pos = editorView.posAtCoords({ x: event.clientX, y: event.clientY });
		if (pos == null) return false;
		requestDetail(editorView.state.doc.lineAt(pos).number);
		return true;
	}

	// Only the implementation detail of "which line is the hover decoration currently on" — never
	// read by the template, so a plain closure variable rather than `$state`.
	let hoveredLineNumber: number | null = null;

	function hoverDecorationFor(editorView: EditorView, lineNumber: number | null): Extension {
		if (lineNumber == null || lineNumber < 1 || lineNumber > editorView.state.doc.lines) return [];
		const line = editorView.state.doc.line(lineNumber);
		const builder = new RangeSetBuilder<Decoration>();
		builder.add(line.from, line.from, Decoration.line({ class: "cm-todotxt-hover" }));
		return EditorView.decorations.of(builder.finish());
	}

	function clearHover(editorView: EditorView) {
		if (hoveredLineNumber === null) return;
		hoveredLineNumber = null;
		editorView.dispatch({ effects: hoverLineCompartment.reconfigure([]) });
	}

	function handleMouseMove(event: MouseEvent, editorView: EditorView): boolean {
		const pos = editorView.posAtCoords({ x: event.clientX, y: event.clientY });
		const lineNumber = pos == null ? null : editorView.state.doc.lineAt(pos).number;
		if (lineNumber === hoveredLineNumber) return false;
		hoveredLineNumber = lineNumber;
		editorView.dispatch({ effects: hoverLineCompartment.reconfigure(hoverDecorationFor(editorView, lineNumber)) });
		return false;
	}

	/** Computes the delta and applies it via the exact same `Apply` path every other edit in this
	 * app uses (tasks/desktop-raw-mode's rawMode.ts module doc) — never a whole-string write. A
	 * no-op save short-circuits before even building a delta. */
	async function applyBufferDelta(targetPath: string, base: string, next: string): Promise<void> {
		if (isNoOpSave(base, next)) return;
		const mutations = computeDelta(base, next);
		if (mutations.length === 0) return;
		await applyMutations(targetPath, mutations);
		// The daemon's own `Change` for `targetPath` repaints the view (and refreshes `baseline`)
		// via `refreshDoc` below — no manual repaint here.
	}

	/** Blur/Cmd-S: commits the buffer against *this* instance's current `path`. (The file-switch
	 * `$effect` below calls `applyBufferDelta` directly instead, against the path being left,
	 * since by the time it runs `path` already holds the destination.) */
	async function commit(): Promise<void> {
		if (!view || !dirty) return;
		const targetPath = path;
		const base = baseline;
		const next = view.state.doc.toString();
		setDirty(false);
		applyEditableGate(); // a pending review that arrived mid-edit is only enforced once clean
		try {
			await applyBufferDelta(targetPath, base, next);
		} catch (e) {
			loadError = String(e);
			await refreshDoc(); // re-sync from the daemon rather than leave a possibly-stale view
		}
	}

	/** Escape: discards (no `Apply`, no op-log entry), reverting the buffer to `baseline`. */
	function discard() {
		if (!view || !dirty) return;
		view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: baseline } });
		setDirty(false);
		applyEditableGate();
	}

	/** Reconfigures `editableCompartment` from `hasPendingReview` — but never while `dirty`: a
	 * pending review that shows up *while the human is already mid-edit* must not yank the buffer
	 * read-only out from under them (that both stops further typing and, via the `blur` a browser
	 * fires when a focused contenteditable turns non-editable, forces a premature `commit()` the
	 * daemon would likely reject anyway, since the task is now under review — see `commit`/
	 * `discard`, which reapply this gate themselves once the edit is no longer dirty). Read via
	 * `untrack` when called from the `$effect` below so this isn't itself a `dirty` dependency —
	 * every keystroke would otherwise re-dispatch a reconfigure for nothing. */
	function applyEditableGate() {
		if (!view) return;
		const blocked = hasPendingReview && !untrack(() => dirty);
		view.dispatch({
			effects: editableCompartment.reconfigure([EditorView.editable.of(!blocked), EditorState.readOnly.of(blocked)])
		});
	}

	async function refreshFilesByPath() {
		const files = await listFiles();
		filesByPath = new Map(files.map((f) => [f.path, f]));
		if (view) view.dispatch({ effects: lineDecoCompartment.reconfigure(lineDecorations(dirOf(path), filesByPath)) });
	}

	/**
	 * Replaces the whole document with fresh `get_file` bytes — the Watch feed is "here's what
	 * changed," not a diff, so we never try to patch the doc from `Change.ops` ourselves
	 * (tasks/desktop-main-view/notes.md). Dispatching one full-range change (rather than
	 * `view.setState`) still lets CM6 map the viewport/decorations/scroll through the edit, so an
	 * in-place edit doesn't visually "reload" the file.
	 */
	async function refreshDoc() {
		if (!view) return;
		// A concurrent `Watch` change (another device, or the file watcher) must never silently
		// overwrite the human's in-progress edit. The conflict banner (a sibling component, fed by
		// the same `Watch` stream) still shows any `needs_review` flag the change raised; this
		// guard only holds off the *document* repaint until the dirty buffer has been committed or
		// discarded.
		if (dirty) return;
		try {
			const contents = await getFile(path);
			loadError = "";
			baseline = contents.text;
			const current = view.state.doc.toString();
			if (current !== contents.text) {
				view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: contents.text } });
			}
		} catch (e) {
			loadError = String(e);
		}
	}

	let unlistenChange: (() => void) | undefined;

	onMount(() => {
		view = new EditorView({ state: EditorState.create({ doc: "", extensions: initialExtensions() }), parent: containerEl });

		let cancelled = false;
		(async () => {
			await refreshFilesByPath();
			await refreshDoc();
			await watchPaths([path]);
			const unlisten = await onDaemonChange((change) => {
				if (change.path === path) refreshDoc();
			});
			if (cancelled) unlisten();
			else unlistenChange = unlisten;
		})();

		return () => {
			cancelled = true;
		};
	});

	onDestroy(() => {
		unlistenChange?.();
		// Best-effort, fire-and-forget commit: a dirty buffer must never just vanish with the
		// component (an unmount going back to the root view is a "switch" too, same as the
		// `$effect` below). The `Apply` call outlives the component; nothing here awaits it, since
		// `onDestroy` can't block teardown.
		if (dirty && view) {
			applyBufferDelta(path, baseline, view.state.doc.toString()).catch(() => {});
		}
		view?.destroy();
	});

	// Reflects a pending-review flag that appears/clears *while already mounted* (entering/leaving
	// read-only) — the initial value is already baked into `initialExtensions()` above.
	$effect(() => {
		hasPendingReview; // dependency: `dirty` itself is read untracked inside — see the doc comment
		applyEditableGate();
	});

	// A future detail view can swap `path` on a mounted FileView (e.g. selecting a different
	// sub-list) — re-baseline against the new file rather than assuming `path` is fixed for life.
	// svelte-ignore state_referenced_locally -- intentional: tracks path's *previous* value to
	// detect a change, not a live derivation of it.
	let mountedPath = path;
	$effect(() => {
		if (path !== mountedPath && view) {
			const oldPath = mountedPath;
			const newPath = path;
			mountedPath = path;

			// A dirty buffer is per-instance, per-file state — commit (or, if unchanged, no-op) the
			// outgoing file's buffer *before* this instance starts showing `newPath`, so it is
			// never carried across the switch. This mirrors `commit` but targets `oldPath`, since
			// by the time this effect runs `path` (and hence a plain `commit()` call) already means
			// the destination, not the file the buffer belongs to.
			const wasDirty = dirty;
			const oldBaseline = baseline;
			const bufferAtSwitch = wasDirty && view ? view.state.doc.toString() : "";
			if (wasDirty) setDirty(false);
			const settle = wasDirty
				? applyBufferDelta(oldPath, oldBaseline, bufferAtSwitch).catch((e) => {
						loadError = String(e);
					})
				: Promise.resolve();

			settle.then(() => {
				if (!view) return;
				view.dispatch({ effects: lineDecoCompartment.reconfigure(lineDecorations(dirOf(newPath), filesByPath)) });
				refreshDoc();
				watchPaths([newPath]);
			});
		}
	});
</script>

<div class="file-view" class:fill style={`--depth: ${depth};`} data-line-count={docLineCount}>
	{#if loadError}
		<p class="error" role="alert">{loadError}</p>
	{/if}

	<div class="editor-shell" bind:this={containerEl}></div>
</div>

<style>
	.file-view {
		position: relative;
		padding-left: calc(var(--depth, 0) * 1rem);
	}

	.file-view.fill {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		height: 100%;
	}

	.error {
		color: var(--color-danger);
		margin: 0 0.5rem 0.4rem;
		font-size: 0.85rem;
	}

	.file-view.fill .editor-shell {
		flex: 1;
		min-height: 0;
	}

	.editor-shell {
		border: 1px solid var(--color-border-subtle);
		border-radius: 6px;
		max-height: 70vh;
		overflow: auto;
	}

	/* Filling mode: no card chrome, no cap — the editor itself is the whole available area. */
	.file-view.fill .editor-shell {
		border: none;
		border-radius: 0;
		max-height: none;
		height: 100%;
	}

	.file-view.fill .editor-shell :global(.cm-editor) {
		height: 100%;
	}
</style>
