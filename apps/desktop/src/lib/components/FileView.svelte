<script lang="ts">
	// THE reusable file-rendering component (tasks/desktop-main-view). Binds to `watch([path])`,
	// rebuilds from `get_file(path)` on each matching `Change`, and renders a read-only CM6
	// `EditorView` with real line numbers (blanks included — design §2.6: blanks are entries).
	// `depth` is a hint only (nested indentation); nothing here assumes `depth === 0` — a future
	// detail-view task mounts this again at `depth + 1` for a `ref:` directory's todo.txt.
	//
	// CM6 basics: https://codemirror.net/docs/ref/
	import { onDestroy, onMount } from "svelte";
	import { Compartment, EditorState } from "@codemirror/state";
	import { EditorView, keymap, lineNumbers } from "@codemirror/view";
	import { defaultKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import {
		applyMutations,
		getFile,
		listFiles,
		onDaemonChange,
		watchPaths,
		type FileInfo,
		type TaskRef
	} from "$lib/daemon";
	import { dirOf } from "$lib/todotxt/lineInfo";
	import { idTagsHidden, idTagsVisible, lineDecorations, mainViewBaseTheme } from "$lib/todotxt/decorations";
	import type { EditRequest } from "$lib/todotxt/editRequest";
	import { canEnterRawMode, computeDelta, isNoOpSave } from "$lib/todotxt/rawMode";
	import { flagsForPath, pendingConflicts } from "$lib/stores/conflicts";
	import type { DetailParams } from "$lib/types";
	import EditPopover from "./EditPopover.svelte";

	interface Props {
		path: string;
		depth: number;
		/** Stretches the editor to fill its container's height instead of the default capped
		 * card (see `.editor-shell` below) — the root view (`MainView`) wants the editor to occupy
		 * all remaining space; a nested detail-view sub-list sits among other sections on a normal
		 * scrolling page and keeps the capped look. */
		fill?: boolean;
		/** Optional: lets a parent (MainView) host the popover instead. Falls back to a local one. */
		onEditRequest?: (req: EditRequest) => void;
		/** Double-click (or Cmd/Ctrl+Enter) on a line (tasks/desktop-detail-view, plan §3.2): opens
		 * the detail view for that line, whether or not it has a `ref:` tag yet — a task with none
		 * still lazily creates one on the first notes/sub-list edit (plan §3.2.4). Optional so a
		 * `FileView` used somewhere that never wants a detail view (there is none today, but the
		 * component itself shouldn't assume one always exists) doesn't have to pass a no-op. */
		onDetailRequest?: (params: DetailParams) => void;
	}

	let { path, depth, fill = false, onEditRequest, onDetailRequest }: Props = $props();

	let containerEl: HTMLDivElement | undefined = $state();
	let view: EditorView | undefined;
	let showIdTags = $state(false);
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let hoveredLine = $state<{ number: number; top: number } | null>(null);
	let localPopover = $state<EditRequest | null>(null);
	let addLineValue = $state("");
	let loadError = $state("");

	// Raw mode (tasks/desktop-raw-mode, plan §3.2/§7): the whole document becomes an editable
	// buffer on Cmd/Ctrl+E; see rawMode.ts for why exit submits a line-level delta rather than a
	// whole-string replace. `raw` is per-`FileView`-instance state, never carried across a `path`
	// swap (breadcrumb nav re-renders this same instance with a new `path` — see the `$effect`
	// below) or across an unmount — both are handled explicitly rather than left to fall out of
	// Svelte's own component teardown, per notes.md's "never carry an unsaved raw buffer across
	// file switches" invariant.
	let raw = $state(false);
	let rawBaseline = "";
	// Pending needs_review flags for this instance's own `path` (design §4.7's store, already fed
	// by `ConflictBanner`'s `Watch` subscription) — read here only to refuse *entering* raw mode
	// while one is showing (notes.md: "the document in raw mode is the reconciled projection").
	const hasPendingReview = $derived(flagsForPath($pendingConflicts, path).length > 0);

	// Per-instance reconfigurable slots (never shared across FileView instances — see
	// $lib/todotxt/decorations.ts) so toggling "show id: tags" or refreshing ref: progress is a
	// cheap dispatch, not a full document rebuild.
	const idTagsCompartment = new Compartment();
	const lineDecoCompartment = new Compartment();
	const editableCompartment = new Compartment();
	// `EditorView.editorAttributes` (not `contentAttributes`) so `data-raw` lands on the whole
	// `.cm-editor` box — the element the visible border/background style below targets.
	const rawAttrCompartment = new Compartment();

	function initialExtensions() {
		return [
			lineNumbers(),
			EditorView.lineWrapping,
			editableCompartment.of([EditorView.editable.of(false), EditorState.readOnly.of(true)]),
			rawAttrCompartment.of(EditorView.editorAttributes.of({})),
			// `Mod-Enter` (Cmd+Enter on macOS, Ctrl+Enter elsewhere — CM6's own convention:
			// https://codemirror.net/docs/ref/#commands) is the keyboard equivalent of a
			// double-click, both opening the detail view for the line under the caret (plan §3.2).
			// `Mod-e` toggles raw mode (https://codemirror.net/docs/ref/#view.keymap); `Mod-s`
			// commits it (`e.preventDefault` happens implicitly — a bound key returning `true`
			// stops CM6 from letting the browser's own Save dialog see it, per `keymap`'s docs).
			// `Escape` discards it — everywhere else Escape is unbound today, so this never shadows
			// another command.
			keymap.of([
				{ key: "Mod-Enter", run: openDetailAtSelection },
				{ key: "Mod-e", run: toggleRaw },
				{
					key: "Mod-s",
					run: () => {
						if (!raw) return false;
						commitRaw();
						return true;
					}
				},
				{
					key: "Escape",
					run: () => {
						if (!raw) return false;
						discardRaw();
						return true;
					}
				},
				...defaultKeymap
			]),
			todotxtLanguage,
			mainViewBaseTheme,
			idTagsCompartment.of(idTagsHidden), // hidden by default — §3.1
			lineDecoCompartment.of(lineDecorations(dirOf(path), filesByPath)),
			EditorView.domEventHandlers({
				mousemove: handleMouseMove,
				mouseleave: () => {
					hoveredLine = null;
				},
				dblclick: handleDblClick,
				// Blur commits (notes.md: "on blur or Cmd/Ctrl+S the buffer goes through the
				// reconciler"). CM6's `blur` domEventHandler fires on the real DOM blur of the
				// content element, i.e. focus actually left the editor — not on the transient focus
				// shuffles a single click within it can cause.
				blur: () => {
					if (raw) commitRaw();
					return false;
				}
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

	function handleMouseMove(event: MouseEvent, editorView: EditorView): boolean {
		const pos = editorView.posAtCoords({ x: event.clientX, y: event.clientY });
		if (pos == null) {
			hoveredLine = null;
			return false;
		}
		const line = editorView.state.doc.lineAt(pos);
		const coords = editorView.coordsAtPos(line.from);
		const containerTop = containerEl?.getBoundingClientRect().top ?? 0;
		hoveredLine = { number: line.number, top: (coords?.top ?? 0) - containerTop };
		return false;
	}

	/** Reconfigures the CM6 compartments only — never touches `raw` itself, so both `enterRaw` and
	 * the two exit paths (`commitRaw`/`discardRaw`) can share it. */
	function setRawVisuals(on: boolean) {
		if (!view) return;
		view.dispatch({
			effects: [
				editableCompartment.reconfigure([EditorView.editable.of(on), EditorState.readOnly.of(!on)]),
				rawAttrCompartment.reconfigure(EditorView.editorAttributes.of(on ? { "data-raw": "true" } : {}))
			]
		});
	}

	function enterRaw() {
		if (!view) return;
		rawBaseline = view.state.doc.toString();
		raw = true;
		setRawVisuals(true);
		view.focus();
	}

	/** `Mod-e`'s keymap `run`: refuses to enter while a conflict is pending (notes.md); toggling
	 * off is exactly a commit, so pressing `Mod-e` again is equivalent to blur/Cmd-S. */
	function toggleRaw(): boolean {
		if (!view) return false;
		if (raw) {
			commitRaw();
		} else {
			if (!canEnterRawMode(hasPendingReview)) return true; // refuse; swallow the keystroke
			enterRaw();
		}
		return true;
	}

	/** Esc: discards (notes.md — no `Apply`, no op-log entry), reverting the buffer to the
	 * baseline it started from. */
	function discardRaw() {
		if (!view || !raw) return;
		if (view.state.doc.toString() !== rawBaseline) {
			view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: rawBaseline } });
		}
		setRawVisuals(false);
		raw = false;
	}

	/** Computes the delta and applies it via the exact same `Apply` path every other edit in this
	 * app uses (see rawMode.ts's module doc) — never a whole-string write, never a second write
	 * path. A no-op save short-circuits before even building a delta. */
	async function submitRawEdit(targetPath: string, baseline: string, next: string): Promise<void> {
		if (isNoOpSave(baseline, next)) return;
		const mutations = computeDelta(baseline, next);
		if (mutations.length === 0) return;
		await applyMutations(targetPath, mutations);
		// The daemon's own `Change` for `targetPath` repaints the (now read-only) view via
		// `refreshDoc` below — no manual repaint here, same convention `saveLocalEdit` follows.
	}

	/** Blur/Cmd-S/toggle-off: commits raw mode's buffer against *this* instance's current `path`.
	 * (The file-switch `$effect` below calls `submitRawEdit` directly instead, against the path
	 * being left, since by the time it runs `path` already holds the destination.) */
	async function commitRaw(): Promise<void> {
		if (!view || !raw) return;
		const targetPath = path;
		const baseline = rawBaseline;
		const next = view.state.doc.toString();
		setRawVisuals(false);
		raw = false;
		try {
			await submitRawEdit(targetPath, baseline, next);
		} catch (e) {
			loadError = String(e);
			await refreshDoc(); // re-sync from the daemon rather than leave a possibly-stale view
		}
	}

	/** The raw line text and a `TaskRef` for the currently hovered line, or `null`. */
	function hoveredLineRef(): { text: string; taskRef: TaskRef } | null {
		if (!view || !hoveredLine) return null;
		const line = view.state.doc.line(hoveredLine.number);
		const idMatch = /\bid:(\S+)/.exec(line.text);
		return {
			text: line.text,
			taskRef: { line_number: line.number, task_id: idMatch ? idMatch[1] : "" }
		};
	}

	function openPencil(anchor: HTMLElement) {
		const ref = hoveredLineRef();
		if (!ref) return;
		const req: EditRequest = { path, initialLine: ref.text, taskRef: ref.taskRef, anchor };
		if (onEditRequest) onEditRequest(req);
		else localPopover = req;
	}

	function closeLocalPopover() {
		localPopover = null;
	}

	async function saveLocalEdit(newLine: string) {
		if (!localPopover) return;
		const { taskRef } = localPopover;
		localPopover = null;
		await applyMutations(path, [{ kind: "edit", task: taskRef, new_line: newLine }]);
		// No manual repaint: the daemon's own `Change` for this path drives `refreshDoc` below.
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
		// Raw mode owns the buffer while it's open — a concurrent `Watch` change (another device,
		// or the file watcher) must never silently overwrite the human's in-progress edit
		// (notes.md: "never silently overwritten"). The conflict banner (a sibling component, fed
		// by the same `Watch` stream) still shows any `needs_review` flag the change raised; this
		// guard only holds off the *document* repaint until the raw buffer itself has been
		// committed or discarded.
		if (raw) return;
		try {
			const contents = await getFile(path);
			loadError = "";
			const current = view.state.doc.toString();
			if (current !== contents.text) {
				view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: contents.text } });
			}
		} catch (e) {
			loadError = String(e);
		}
	}

	async function addLine() {
		const line = addLineValue.trim();
		if (!line) return;
		addLineValue = "";
		await applyMutations(path, [{ kind: "add", line }]);
		// The daemon stamps the creation date/id: and its Change repaints us — see refreshDoc.
	}

	function toggleIdTags() {
		showIdTags = !showIdTags;
		if (view) {
			view.dispatch({ effects: idTagsCompartment.reconfigure(showIdTags ? idTagsVisible : idTagsHidden) });
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
		// Best-effort, fire-and-forget commit (same pattern `setMainPopoverDirty`'s doc comment
		// describes): a raw buffer must never just vanish with the component (notes.md "never
		// carry an unsaved raw buffer across file switches" — an unmount going back to the root
		// view is a switch too). The `Apply` call outlives the component; nothing here awaits it,
		// since `onDestroy` can't block teardown.
		if (raw && view) {
			submitRawEdit(path, rawBaseline, view.state.doc.toString()).catch(() => {});
		}
		view?.destroy();
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

			// Raw mode is per-instance, per-file state — commit (or, if unchanged, no-op) the
			// outgoing file's buffer *before* this instance starts showing `newPath`, so it is
			// never carried across the switch (notes.md's explicit invariant). This mirrors
			// `commitRaw` but targets `oldPath`, since by the time this effect runs `path` (and
			// hence a plain `commitRaw()` call) already means the destination, not the file the
			// buffer belongs to.
			const wasRaw = raw;
			const baseline = rawBaseline;
			const bufferAtSwitch = wasRaw && view ? view.state.doc.toString() : "";
			if (wasRaw) {
				setRawVisuals(false);
				raw = false;
			}
			const settle = wasRaw
				? submitRawEdit(oldPath, baseline, bufferAtSwitch).catch((e) => {
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

<div class="file-view" class:fill style={`--depth: ${depth};`}>
	<header class="file-view-header">
		<label>
			<input type="checkbox" checked={showIdTags} onchange={toggleIdTags} />
			Show <code>id:</code> tags
		</label>
		<!-- Raw mode badge (plan §3.3 accessibility floor: colour is never the only signal — see
		     the `[data-raw]` border/background rule below for the other half of that). `aria-pressed`
		     mirrors `raw` for a screen reader; the button is a second, pointer-reachable way to
		     toggle raw mode alongside `Mod-e`, disabled while a conflict must be resolved first. -->
		<button
			type="button"
			class="raw-toggle"
			class:active={raw}
			aria-pressed={raw}
			disabled={!raw && hasPendingReview}
			title={!raw && hasPendingReview ? "Resolve the pending conflict before editing raw" : "Toggle raw mode (Mod-E)"}
			onclick={toggleRaw}
		>
			{raw ? "Raw mode: on" : "Raw mode"}
		</button>
		{#if loadError}
			<span class="error" role="alert">{loadError}</span>
		{/if}
	</header>

	<div class="editor-wrap">
		<div class="editor-shell" bind:this={containerEl}></div>

		{#if hoveredLine}
			<button
				class="pencil"
				style={`top: ${hoveredLine.top}px;`}
				aria-label={`Edit line ${hoveredLine.number}`}
				onclick={(e) => openPencil(e.currentTarget)}
			>
				&#9998;
			</button>
		{/if}
	</div>

	<div class="add-line-row">
		<span class="add-line-affordance">+</span>
		<input
			type="text"
			placeholder="Add a line…"
			bind:value={addLineValue}
			onkeydown={(e) => e.key === "Enter" && addLine()}
		/>
	</div>

	{#if localPopover}
		<EditPopover
			{path}
			initialLine={localPopover.initialLine}
			taskRef={localPopover.taskRef}
			anchor={localPopover.anchor}
			onSave={saveLocalEdit}
			onCancel={closeLocalPopover}
		/>
	{/if}
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

	.file-view-header {
		display: flex;
		align-items: center;
		gap: 1rem;
		font-size: 0.85rem;
		margin-bottom: 0.4rem;
		padding: 0 0.5rem;
	}

	.error {
		color: #b91c1c;
	}

	.raw-toggle {
		font-size: 0.8rem;
		border: 1px solid #d1d5db;
		border-radius: 999px;
		background: transparent;
		padding: 0.15rem 0.6rem;
		cursor: pointer;
	}

	.raw-toggle.active {
		border-color: #b45309;
		background: #fef3c7;
		font-weight: 600;
	}

	.raw-toggle:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	/* Raw mode's visible state (plan §3.3: colour is never the only signal) — border *and*
	   background change together, matched by `.raw-toggle.active` above and the `aria-pressed`
	   badge for anyone not relying on colour/shape at all. `:global` since CM6 owns this element's
	   markup, not this component's own template. */
	.editor-wrap :global(.cm-editor[data-raw]) {
		border: 2px solid #b45309;
		background: #fffbeb;
	}

	.editor-wrap {
		position: relative;
	}

	.file-view.fill .editor-wrap {
		flex: 1;
		min-height: 0;
	}

	.editor-shell {
		border: 1px solid #e5e7eb;
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

	.pencil {
		position: absolute;
		right: 0.5rem;
		transform: translateY(-50%);
		border: none;
		background: transparent;
		cursor: pointer;
		font-size: 0.9rem;
		line-height: 1;
		padding: 0.15rem;
	}

	.add-line-row {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.35rem 0.6rem;
		opacity: 0.7;
	}

	.add-line-row input {
		flex: 1;
		border: none;
		font: inherit;
		background: transparent;
	}

	.add-line-row input:focus {
		outline: none;
	}
</style>
