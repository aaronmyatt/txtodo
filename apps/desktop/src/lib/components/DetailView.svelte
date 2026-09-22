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
	import { onDestroy, onMount } from "svelte";
	import { EditorState } from "@codemirror/state";
	import { EditorView, keymap } from "@codemirror/view";
	import { defaultKeymap, history as cmHistory, historyKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { idTagsHidden } from "$lib/todotxt/decorations";
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
	import { findRefTag, joinPath, refDirFor } from "$lib/todotxt/lineInfo";
	import { notesLayout, notesMode, subListMode } from "$lib/detailSections";
	import { workspaceLayoutStore } from "$lib/stores/workspaces";
	import { lineOfTask, taskIdAt } from "$lib/todotxt/taskIds";
	import type { DetailParams } from "$lib/types";
	import Breadcrumb from "./Breadcrumb.svelte";
	import ConflictBanner from "./ConflictBanner.svelte";
	import FileView from "./FileView.svelte";
	import NotesEditor from "./NotesEditor.svelte";

	let {
		steps,
		onNavigateInto,
		onNavigateToLevel
	}: {
		/** The full navigation stack; `steps[steps.length - 1]` is this instance's own level. */
		steps: DetailParams[];
		/** A sub-list line was double-clicked: push one more level. */
		onNavigateInto: (params: DetailParams) => void;
		/** A breadcrumb crumb was clicked: truncate the stack to this length (0 = home/root). */
		onNavigateToLevel: (stackLength: number) => void;
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
		const next = parentView.state.doc.toString();
		const base = parentBaseline;
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
							run: () => {
								discardParentEdit();
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

	<ConflictBanner path={current.file} />

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
	.detail-view {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		padding: 2rem;
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
		font-size: 1rem;
	}

	.parent-line :global(.cm-editor) {
		outline: none;
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
