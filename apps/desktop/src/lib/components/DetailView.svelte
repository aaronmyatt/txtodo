<script lang="ts">
	// Detail view (tasks/desktop-detail-view, plan §3.2, design §7): pinned parent line, breadcrumb,
	// footer, the recursive sub-list when it has tasks, and the notes editor always — under the
	// sub-list as a collapsible section that starts open when notes.md has text (task
	// desktop-notes-hidden: "exactly one of" hid every ref's notes, since each has both files).
	// A page, not a modal — `MainView.svelte` swaps this in for its
	// whole content area rather than overlaying it (plan §3.3).
	//
	// Everything shown here comes from the daemon's tree/`Watch` (design §2.6, plan §3.2 rule 2):
	// this component never resolves a `ref:` slug or creates a directory itself. It only reads the
	// parent line's *own* `ref:` tag (via the same `findRefTag` the main view's decorations use) to
	// know where the sub-list/notes for THIS task already live, if they do.
	import { leaveEditor } from "$lib/escape";
	import { onDestroy, onMount } from "svelte";
	import { EditorState } from "@codemirror/state";
	import { EditorView, keymap } from "@codemirror/view";
	import { defaultKeymap, history as cmHistory, historyKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { idTagsHidden, refTagsHidden } from "$lib/todotxt/decorations";
	import { keepRefTag } from "$lib/todotxt/keepRefTag";
	import { singleLineFilter } from "$lib/todotxt/singleLineFilter";
	import {
		applyMutations,
		getFile,
		listFiles,
		onDaemonChange,
		refDir as refDirRpc,
		watch,
		workspaceRoot,
		type FileInfo,
		type NotesDoc,
		type TaskRef
	} from "$lib/daemon";
	import { localToday } from "./editPopoverLogic";
	import { copyText, otherLists } from "$lib/todotxt/moveCopy";
	import { popChange } from "$lib/stores/changes";
	import { movePicker } from "$lib/stores/movePicker";
	import { findRefTag, joinPath, refDirFor } from "$lib/todotxt/lineInfo";
	import { notesLayout, notesMode, subListMode } from "$lib/detailSections";
	import { workspaceLayoutStore } from "$lib/stores/workspaces";
	import { lineOfTask, taskIdAt } from "$lib/todotxt/taskIds";
	import type { DetailParams } from "$lib/types";
	import { breakdownFields, type BreakdownField } from "$lib/wasmCore";
	import Breadcrumb from "./Breadcrumb.svelte";
	import ConflictBanner from "./ConflictBanner.svelte";
	import FileView from "./FileView.svelte";
	import NotesEditor from "./NotesEditor.svelte";

	let {
		steps,
		onNavigateInto,
		onNavigateToLevel,
		rootPath = ""
	}: {
		/** The full navigation stack; `steps[steps.length - 1]` is this instance's own level. */
		steps: DetailParams[];
		/** A sub-list line was double-clicked: push one more level. */
		onNavigateInto: (params: DetailParams) => void;
		/** A breadcrumb crumb was clicked: truncate the stack to this length (0 = home/root). */
		onNavigateToLevel: (stackLength: number) => void;
		/** The root list the buffer above already shows; its conflict banner is not repeated here. */
		rootPath?: string;
	} = $props();

	const current = $derived(steps[steps.length - 1]);
	const depth = $derived(steps.length);

	let parentLine = $state("");
	let parentTaskId = $state("");
	let saveError = $state("");
	let loadError = $state("");
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let workspaceRootPath = $state("");
	// What the daemon said about notes.md: the path it actually read (shown in the footer, so a
	// client/daemon layout disagreement is visible) and whether the collapsed section starts open.
	let notesPath = $state("");
	let notesOpen = $state(false);
	let notesSeen = false;

	// The pinned parent line edits directly, the same as FileView's own document (click, type,
	// blur/Enter commits) — no popover. `parentDirty`/`parentBaseline` mirror FileView.svelte's
	// `dirty`/`baseline` pair, scoped to this one line instead of a whole file.
	let parentEditorEl: HTMLDivElement | undefined;
	let parentView: EditorView | undefined;
	let parentDirty = $state(false);
	let parentBaseline = "";

	// The line the pinned task is on now. It starts as the line the human opened, then follows the
	// task by id: completing moves the line to the bottom of its file (task complete-to-bottom),
	// and any other reorder would strand a view that only knew a line number.
	let parentLineNumber = $state(0);
	const parentTaskRef = $derived<TaskRef>({ line_number: parentLineNumber || current.line, task_id: parentTaskId });
	const refTag = $derived(findRefTag(parentLine));
	const refDir = $derived(refTag ? refDirFor($workspaceLayoutStore, current.file, refTag.slug) : null);
	const subListPath = $derived(refDir ? joinPath(refDir, "todo.txt") : null);
	const subListInfo = $derived(subListPath ? (filesByPath.get(subListPath) ?? null) : null);
	const absoluteRefDir = $derived(
		refDir && workspaceRootPath ? `${workspaceRootPath}/${refDir}` : (workspaceRootPath ?? "")
	);
	const subListTotal = $derived(subListInfo?.total ?? 0);
	const hasSubList = $derived(subListMode(subListTotal) === "tasks");
	// The daemon answered `GetNotes` with no path although the line has a `ref:` tag: it resolved
	// the folder elsewhere (an older daemon); an empty editor here would write a second notes.md.
	let notesPathMissing = $state(false);
	const notesState = $derived(
		notesMode({ parentLine, parentTaskId, hasRefTag: refTag !== null, notesPathMissing })
	);

	// The parent line field by field (root todo "the detail view task input box obscures…"; manifest
	// `detail.breakdown`): core's rows through wasm, behind a toggle. A button, not the TUI's `v`:
	// this panel's editors hold the keyboard, where a letter types.
	let fieldsOpen = $state(false);
	let fields = $state<BreakdownField[]>([]);
	$effect(() => {
		const line = parentLine;
		if (!fieldsOpen) return;
		void breakdownFields(line).then((rows) => {
			if (line === parentLine) fields = rows;
		});
	});

	// Starting a sub-list on a task that has none yet (task desktop-sublist-start): the section
	// shows one add-line input instead of a FileView. Nothing is written until the first submit.
	let firstSubTask = $state("");
	let startingSubList = $state(false);
	let subListError = $state("");

	/** First submit: `refDir(ensure)` claims the directory and the `ref:` tag (one op batch,
	 * daemon-side), then the line goes into `<dir>/todo.txt` through the same `applyMutations`
	 * every add uses — the daemon registers that not-yet-existing list on its first `Add`
	 * (`crates/txtodo-daemon/src/apply_route.rs::actor_or_new_list`). The daemon's own `Change`
	 * then repaints the parent line with its new tag and `refreshFiles` finds the sub-list, so
	 * the FileView takes over from this input. */
	async function startSubList(e: SubmitEvent) {
		e.preventDefault();
		const line = firstSubTask.trim();
		if (!line || startingSubList) return;
		startingSubList = true;
		subListError = "";
		try {
			const info = await refDirRpc(current.file, parentTaskRef, true);
			await applyMutations(joinPath(info.dir, "todo.txt"), [{ kind: "add", line }]);
			firstSubTask = "";
			await refreshFiles();
		} catch (err) {
			subListError = String(err);
		} finally {
			startingSubList = false;
		}
	}

	function onNotesLoaded(doc: NotesDoc) {
		notesPath = doc.path;
		notesPathMissing = doc.path === "";
		// Open once, on the first answer: a later reload must not fight the human's own toggle.
		if (!notesSeen) {
			notesSeen = true;
			notesOpen = notesLayout(subListTotal, doc.text).open;
		}
	}
	const allSubTasksDone = $derived(
		subListInfo !== null && subListInfo.total > 0 && subListInfo.done === subListInfo.total
	);

	async function loadParentLine() {
		try {
			const contents = await getFile(current.file);
			loadError = "";
			const lines = contents.text.split("\n");
			// First load: the id of the line that was opened. Later loads: wherever that id is now.
			const lineNumber = lineOfTask(contents, parentTaskId, parentLineNumber || current.line);
			parentLineNumber = lineNumber;
			parentLine = lines[lineNumber - 1] ?? "";
			parentTaskId = taskIdAt(contents, lineNumber);
		} catch (e) {
			loadError = String(e);
		}
	}

	async function refreshFiles() {
		filesByPath = new Map((await listFiles()).map((f) => [f.path, f]));
	}

	/** Pushes fresh `parentLine` content into the CM6 doc, unless a local edit is in progress
	 * (mirrors FileView.svelte's `refreshDoc` guard: a concurrent `Watch` change must never
	 * silently overwrite the human's in-progress edit). Always updates `parentBaseline`, even
	 * while dirty, so a later commit diffs against the true last-known-good text. */
	function syncParentDoc(text: string) {
		parentBaseline = text;
		if (!parentView || parentDirty) return;
		if (parentView.state.doc.toString() !== text) {
			parentView.dispatch({ changes: { from: 0, to: parentView.state.doc.length, insert: text } });
		}
	}

	async function commitParentEdit() {
		if (!parentView || !parentDirty) return;
		const base = parentBaseline;
		// The hidden `ref:` survives any edit here (`keepRefTag`): it is the link to this panel.
		const next = keepRefTag(base, parentView.state.doc.toString());
		parentDirty = false;
		if (next === base) return;
		saveError = "";
		try {
			await applyMutations(current.file, [{ kind: "edit", task: parentTaskRef, new_line: next }]);
			// The daemon's own `Change` repaints `parentLine` (loadParentLine), which flows back
			// through `syncParentDoc` above — no manual repaint here.
		} catch (e) {
			saveError = String(e);
		}
	}

	function discardParentEdit() {
		if (!parentView || !parentDirty) return;
		parentView.dispatch({ changes: { from: 0, to: parentView.state.doc.length, insert: parentBaseline } });
		parentDirty = false;
	}

	// Move or copy this task to another list (task add-the-ability-to-move-or-copy-a-task-t).
	const targets = $derived(otherLists(filesByPath.values(), current.file));
	let moveCopyNote = $state("");

	/** The daemon's cross-file `Move`: the task keeps its id and its `ref:` folder goes with it.
	 * The line has left this list, so the view goes back a level. Not on `u`'s stack: an Undo
	 * there would put back only this list's half. */
	async function moveTo(dest: string) {
		if (!dest) return;
		saveError = "";
		try {
			await applyMutations(current.file, [{ kind: "move", task: parentTaskRef, to_path: dest }]);
			popChange();
			onNavigateToLevel(steps.length - 1);
		} catch (e) {
			saveError = String(e);
		}
	}

	/** A new task at the end of `dest`, without this one's `id:` and `ref:`. */
	async function copyTo(dest: string) {
		if (!dest) return;
		saveError = "";
		try {
			await applyMutations(dest, [{ kind: "add", line: copyText(parentLine) }]);
			moveCopyNote = `Copied to ${dest}`;
		} catch (e) {
			saveError = String(e);
		}
	}

	// List mode's `m`/`c` opened this detail to pick a list: focus that picker once it shows.
	let moveSelect: HTMLSelectElement | undefined = $state();
	let copySelect: HTMLSelectElement | undefined = $state();
	$effect(() => {
		const intent = $movePicker;
		const select = intent === "move" ? moveSelect : intent === "copy" ? copySelect : undefined;
		if (!select) return;
		movePicker.set("");
		select.focus();
	});

	/** A `<select>` that acts on pick, then shows its prompt again. */
	function picked(e: Event, act: (dest: string) => Promise<void>) {
		const select = e.currentTarget as HTMLSelectElement;
		const dest = select.value;
		select.value = "";
		void act(dest);
	}

	async function markParentDone() {
		saveError = "";
		try {
			await applyMutations(current.file, [
				{ kind: "complete", task: parentTaskRef, today: localToday() }
			]);
		} catch (e) {
			saveError = String(e);
		}
	}

	let unlistenChange: (() => void) | undefined;

	onMount(() => {
		let cancelled = false;
		(async () => {
			const [root] = await Promise.all([workspaceRoot(), refreshFiles()]);
			workspaceRootPath = root;
			await loadParentLine();
			await watch();
			const unlisten = await onDaemonChange((change) => {
				if (change.path === current.file) loadParentLine();
				refreshFiles();
			});
			if (cancelled) unlisten();
			else unlistenChange = unlisten;
		})();
		return () => {
			cancelled = true;
		};
	});

	onMount(() => {
		parentView = new EditorView({
			state: EditorState.create({
				doc: parentLine,
				extensions: [
					todotxtLanguage,
					idTagsHidden,
					refTagsHidden,
					// A long parent line wraps onto more rows instead of scrolling sideways under a
					// scrollbar that covered the text (root todo "the detail view task input box
					// obscures…"). Still one logical line: `singleLineFilter` and Enter keep it so.
					// https://codemirror.net/docs/ref/#view.EditorView^lineWrapping
					EditorView.lineWrapping,
					singleLineFilter(),
					cmHistory(),
					// `Prec.highest` isn't needed here since these are the only Enter/Escape bindings —
					// `defaultKeymap`'s own Enter (insert newline) never gets a chance to run first as
					// long as this array comes before it (CM6 keymaps are tried in extension order).
					keymap.of([
						{
							key: "Enter",
							preventDefault: true,
							run: () => {
								commitParentEdit();
								return true;
							}
						},
						{
							key: "Escape",
							preventDefault: true,
							run: (v) => {
								discardParentEdit();
								leaveEditor(v.contentDOM);
								return true;
							}
						},
						...historyKeymap,
						...defaultKeymap
					]),
					EditorView.domEventHandlers({
						blur: () => {
							if (parentDirty) commitParentEdit();
							return false;
						}
					}),
					EditorView.updateListener.of((u) => {
						if (u.docChanged) parentDirty = u.state.doc.toString() !== parentBaseline;
					})
				]
			}),
			parent: parentEditorEl
		});
		return () => {
			parentView?.destroy();
		};
	});

	onDestroy(() => {
		unlistenChange?.();
		// Best-effort, fire-and-forget commit (same pattern FileView.svelte's own onDestroy
		// follows): a dirty parent-line edit must never just vanish with the component.
		if (parentDirty && parentView) {
			const next = parentView.state.doc.toString();
			applyMutations(current.file, [{ kind: "edit", task: parentTaskRef, new_line: next }]).catch(() => {});
		}
	});

	$effect(() => {
		syncParentDoc(parentLine);
	});

	// Re-baseline when the human navigates to a different level of an already-mounted DetailView
	// (MainView keeps one DetailView instance alive across pushes — see its template).
	// svelte-ignore state_referenced_locally -- intentional: tracks current.file's *previous*
	// value to detect a change, not a live derivation of it (same pattern as FileView.svelte).
	// The step, not only its file: two levels can share a file and differ in the line. A new step
	// is a new task, so the id and line this instance was following are dropped first; otherwise
	// `loadParentLine` would look for the old task's id in the new step's file.
	const stepKey = (step: DetailParams) => `${step.file}\n${step.line}`;
	// svelte-ignore state_referenced_locally -- intentional, as the note above says
	let watchedFor = stepKey(current);
	$effect(() => {
		if (stepKey(current) !== watchedFor) {
			watchedFor = stepKey(current);
			parentTaskId = "";
			parentLineNumber = 0;
			loadParentLine();
			watch();
		}
	});
</script>

<div class="detail-view">
	<header class="detail-header">
		<button type="button" class="back" onclick={() => onNavigateToLevel(steps.length - 1)} aria-label="Back">
			&lsaquo;
		</button>
		<Breadcrumb {steps} onNavigate={onNavigateToLevel} />
	</header>

	{#if loadError}
		<p class="error" role="alert">{loadError}</p>
	{/if}

	{#if current.file !== rootPath}
		<ConflictBanner path={current.file} />
	{/if}

	<section class="parent" aria-label="Parent task">
		<div class="parent-line" bind:this={parentEditorEl}></div>
		{#if saveError}
			<p class="error" role="alert">{saveError}</p>
		{/if}
		{#if allSubTasksDone}
			<button type="button" class="mark-done-offer" onclick={markParentDone}>
				Mark parent done
			</button>
		{/if}
		{#if targets.length > 0 && parentTaskId}
			<div class="move-copy">
				<select aria-label="Move to another list" bind:this={moveSelect} onchange={(e) => picked(e, moveTo)}>
					<option value="">Move to…</option>
					{#each targets as t (t)}<option value={t}>{t}</option>{/each}
				</select>
				<select aria-label="Copy to another list" bind:this={copySelect} onchange={(e) => picked(e, copyTo)}>
					<option value="">Copy to…</option>
					{#each targets as t (t)}<option value={t}>{t}</option>{/each}
				</select>
				{#if moveCopyNote}<span class="move-copy-note" role="status">{moveCopyNote}</span>{/if}
			</div>
		{/if}
		<button
			type="button"
			class="fields-toggle"
			aria-expanded={fieldsOpen}
			onclick={() => (fieldsOpen = !fieldsOpen)}
		>
			{fieldsOpen ? "Hide fields" : "Fields"}
		</button>
		{#if fieldsOpen}
			<dl class="fields" aria-label="Parent line, field by field">
				{#each fields as field (field.name + field.value)}
					<dt>{field.name}</dt>
					<dd>{field.value}</dd>
				{/each}
			</dl>
		{/if}
	</section>

	<section class="sublist" aria-label="Sub-list">
		{#if hasSubList && subListInfo}
			<h2>{subListInfo.done} of {subListInfo.total} done</h2>
			<FileView path={subListPath ?? ""} {depth} onDetailRequest={onNavigateInto} />
		{:else}
			<h2>Sub-list</h2>
			<!-- No sub-list yet (no `ref:`, or an empty todo.txt): one add-line row. The first
			     submit creates the folder (see `startSubList`); opening the view writes nothing. -->
			<form class="sublist-start" onsubmit={startSubList}>
				<input
					type="text"
					aria-label="Add a sub-task"
					placeholder="Add a sub-task"
					bind:value={firstSubTask}
					disabled={!parentTaskId || startingSubList}
				/>
			</form>
			{#if subListError}
				<p class="error" role="alert">{subListError}</p>
			{/if}
		{/if}
	</section>

	<section class="notes" aria-label="Notes">
		{#snippet notesBody()}
			{#if notesState === "loading"}
				<!-- still loading -->
			{:else if notesState === "unavailable"}
				<p class="empty-state" role="alert">
					Notes could not be loaded: the daemon found no <code>notes.md</code> for this task
					even though its line carries <code>ref:{refTag?.slug}</code>. The running daemon
					is likely older than this app and looks in a different folder; reinstall or restart
					<code>txtodod</code>. Nothing was written.
				</p>
			{:else if notesState === "no-task-id"}
				<!-- `GetNotes`/`EditNotes` resolve a task by task_id alone
				     (crates/txtodo-daemon/src/notes.rs::locate_task), never by line_number. The id comes
				     from `GetFile`'s `task_ids` (`taskIdAt`, task sidecar-task-ids), so a Sidecar line
				     with no `id:` tag resolves too. This branch is left for a daemon older than that
				     field reading a Sidecar workspace: no id in the reply, none in the text. -->
				<p class="empty-state">
					Notes aren't available for this task: the running daemon did not say which task this
					line is. It is likely an older build; reinstall or restart <code>txtodod</code>.
				</p>
			{:else}
				<NotesEditor task={parentTaskRef} onLoaded={onNotesLoaded} />
			{/if}
		{/snippet}
		{#if hasSubList}
			<details bind:open={notesOpen}>
				<summary><h2>Notes</h2></summary>
				{@render notesBody()}
			</details>
		{:else}
			<h2>Notes</h2>
			{@render notesBody()}
		{/if}
	</section>

	<footer class="detail-footer">
		<!-- No `ref:` and no path from the daemon yet: the folder does not exist until the first
		     notes edit creates it, so say that rather than show the workspace root. -->
		<span class="dir">
			{notesPath || (refTag ? absoluteRefDir : "Notes file is created when you type")}
		</span>
	</footer>
</div>

<style>
	/* At least three quarters of the panel's width (2026-10-06 feedback: 48rem left most of a wide
	   window empty); the right padding keeps clear of the panel's close button. */
	.detail-view {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		width: max(75%, min(100%, 48rem));
		margin: 0 auto;
		padding: 1rem 3rem 1rem 1.25rem;
		box-sizing: border-box;
		font-family: var(--font-sans);
	}

	.detail-header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.back {
		font-size: 1.1rem;
		background: transparent;
		border: none;
		cursor: pointer;
	}

	.parent {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 0.5rem;
		border: 1px solid var(--color-border);
		border-radius: 8px;
		padding: 0.75rem 1rem;
		background: var(--color-surface);
	}

	.parent-line {
		width: 100%;
		font-size: var(--editor-font-size, 1rem);
	}

	.parent-line :global(.cm-editor) {
		outline: none;
	}

	.fields-toggle {
		align-self: flex-start;
		font-size: 0.85rem;
	}

	.move-copy {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.85rem;
	}

	.move-copy-note {
		opacity: 0.7;
	}

	/* sqlite `.mode line`: names right-aligned in one column, `=`, the values. */
	.fields {
		display: grid;
		grid-template-columns: max-content 1fr;
		gap: 0.1rem 0.5rem;
		margin: 0;
		font-family: var(--font-mono, ui-monospace, monospace);
		font-size: 0.85rem;
	}

	.fields dt {
		text-align: right;
		color: var(--color-text-muted, inherit);
	}

	.fields dt::after {
		content: " =";
	}

	.fields dd {
		margin: 0;
		overflow-wrap: anywhere;
	}

	.mark-done-offer {
		align-self: flex-start;
	}

	.sublist-start input {
		width: 100%;
		font: inherit;
		padding: 0.4rem 0.6rem;
		border: 1px solid var(--color-border);
		border-radius: 6px;
		background: var(--color-surface);
	}

	.notes summary {
		cursor: pointer;
	}

	.notes summary h2 {
		display: inline;
	}

	.empty-state {
		color: var(--color-text-muted);
		font-style: italic;
	}

	.error {
		color: var(--color-danger);
	}

	.detail-footer {
		font-size: 0.8rem;
		color: var(--color-text-muted);
		border-top: 1px solid var(--color-border-subtle);
		padding-top: 0.5rem;
	}
</style>
