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
	import type { DetailParams } from "$lib/types";
	import EditPopover from "./EditPopover.svelte";

	interface Props {
		path: string;
		depth: number;
		/** Optional: lets a parent (MainView) host the popover instead. Falls back to a local one. */
		onEditRequest?: (req: EditRequest) => void;
		/** Double-click (or Cmd/Ctrl+Enter) on a line (tasks/desktop-detail-view, plan §3.2): opens
		 * the detail view for that line, whether or not it has a `ref:` tag yet — a task with none
		 * still lazily creates one on the first notes/sub-list edit (plan §3.2.4). Optional so a
		 * `FileView` used somewhere that never wants a detail view (there is none today, but the
		 * component itself shouldn't assume one always exists) doesn't have to pass a no-op. */
		onDetailRequest?: (params: DetailParams) => void;
	}

	let { path, depth, onEditRequest, onDetailRequest }: Props = $props();

	let containerEl: HTMLDivElement | undefined = $state();
	let view: EditorView | undefined;
	let showIdTags = $state(false);
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let hoveredLine = $state<{ number: number; top: number } | null>(null);
	let localPopover = $state<EditRequest | null>(null);
	let addLineValue = $state("");
	let loadError = $state("");

	// Per-instance reconfigurable slots (never shared across FileView instances — see
	// $lib/todotxt/decorations.ts) so toggling "show id: tags" or refreshing ref: progress is a
	// cheap dispatch, not a full document rebuild.
	const idTagsCompartment = new Compartment();
	const lineDecoCompartment = new Compartment();

	function initialExtensions() {
		return [
			lineNumbers(),
			EditorView.lineWrapping,
			EditorView.editable.of(false),
			EditorState.readOnly.of(true),
			// `Mod-Enter` (Cmd+Enter on macOS, Ctrl+Enter elsewhere — CM6's own convention:
			// https://codemirror.net/docs/ref/#commands) is the keyboard equivalent of a
			// double-click, both opening the detail view for the line under the caret (plan §3.2).
			keymap.of([{ key: "Mod-Enter", run: openDetailAtSelection }, ...defaultKeymap]),
			todotxtLanguage,
			mainViewBaseTheme,
			idTagsCompartment.of(idTagsHidden), // hidden by default — §3.1
			lineDecoCompartment.of(lineDecorations(dirOf(path), filesByPath)),
			EditorView.domEventHandlers({
				mousemove: handleMouseMove,
				mouseleave: () => {
					hoveredLine = null;
				},
				dblclick: handleDblClick
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
		view?.destroy();
	});

	// A future detail view can swap `path` on a mounted FileView (e.g. selecting a different
	// sub-list) — re-baseline against the new file rather than assuming `path` is fixed for life.
	// svelte-ignore state_referenced_locally -- intentional: tracks path's *previous* value to
	// detect a change, not a live derivation of it.
	let mountedPath = path;
	$effect(() => {
		if (path !== mountedPath && view) {
			mountedPath = path;
			view.dispatch({ effects: lineDecoCompartment.reconfigure(lineDecorations(dirOf(path), filesByPath)) });
			refreshDoc();
			watchPaths([path]);
		}
	});
</script>

<div class="file-view" style={`--depth: ${depth};`}>
	<header class="file-view-header">
		<label>
			<input type="checkbox" checked={showIdTags} onchange={toggleIdTags} />
			Show <code>id:</code> tags
		</label>
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

	.file-view-header {
		display: flex;
		align-items: center;
		gap: 1rem;
		font-size: 0.85rem;
		margin-bottom: 0.4rem;
	}

	.error {
		color: #b91c1c;
	}

	.editor-wrap {
		position: relative;
	}

	.editor-shell {
		border: 1px solid #e5e7eb;
		border-radius: 6px;
		max-height: 70vh;
		overflow: auto;
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
