<script lang="ts">
	// Detail view (tasks/desktop-detail-view, plan §3.2, design §7): pinned parent line, notes
	// editor, recursive sub-list, breadcrumb, footer. A page, not a modal — `MainView.svelte`
	// swaps this in for its whole content area rather than overlaying it (plan §3.3).
	//
	// Everything shown here comes from the daemon's tree/`Watch` (design §2.6, plan §3.2 rule 2):
	// this component never resolves a `ref:` slug or creates a directory itself. It only reads the
	// parent line's *own* `ref:` tag (via the same `findRefTag` the main view's decorations use) to
	// know where the sub-list/notes for THIS task already live, if they do.
	import { onDestroy, onMount } from "svelte";
	import {
		applyMutations,
		getFile,
		listFiles,
		onDaemonChange,
		watchPaths,
		workspaceRoot,
		type FileInfo,
		type TaskRef
	} from "$lib/daemon";
	import { localToday } from "./editPopoverLogic";
	import { dirOf, findRefTag, joinPath } from "$lib/todotxt/lineInfo";
	import type { DetailParams } from "$lib/types";
	import Breadcrumb from "./Breadcrumb.svelte";
	import ConflictBanner from "./ConflictBanner.svelte";
	import EditPopover from "./EditPopover.svelte";
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
	let popoverOpen = $state(false);
	let saveError = $state("");
	let loadError = $state("");
	let filesByPath = $state<Map<string, FileInfo>>(new Map());
	let workspaceRootPath = $state("");

	const parentTaskRef = $derived<TaskRef>({ line_number: current.line, task_id: parentTaskId });
	const refTag = $derived(findRefTag(parentLine));
	const refDir = $derived(refTag ? joinPath(dirOf(current.file), refTag.slug) : null);
	const subListPath = $derived(refDir ? joinPath(refDir, "todo.txt") : null);
	const subListInfo = $derived(subListPath ? (filesByPath.get(subListPath) ?? null) : null);
	const absoluteRefDir = $derived(
		refDir && workspaceRootPath ? `${workspaceRootPath}/${refDir}` : (workspaceRootPath ?? "")
	);
	const allSubTasksDone = $derived(
		subListInfo !== null && subListInfo.total > 0 && subListInfo.done === subListInfo.total
	);

	async function loadParentLine() {
		try {
			const contents = await getFile(current.file);
			loadError = "";
			const lines = contents.text.split("\n");
			const text = lines[current.line - 1] ?? "";
			parentLine = text;
			const idMatch = /\bid:(\S+)/.exec(text);
			parentTaskId = idMatch ? idMatch[1] : "";
		} catch (e) {
			loadError = String(e);
		}
	}

	async function refreshFiles() {
		filesByPath = new Map((await listFiles()).map((f) => [f.path, f]));
	}

	function openPopover() {
		popoverOpen = true;
	}

	function closePopover() {
		popoverOpen = false;
	}

	async function saveParentEdit(newLine: string) {
		await applyMutations(current.file, [{ kind: "edit", task: parentTaskRef, new_line: newLine }]);
		popoverOpen = false;
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
			await watchPaths([current.file]);
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

	onDestroy(() => {
		unlistenChange?.();
	});

	// Re-baseline when the human navigates to a different level of an already-mounted DetailView
	// (MainView keeps one DetailView instance alive across pushes — see its template).
	// svelte-ignore state_referenced_locally -- intentional: tracks current.file's *previous*
	// value to detect a change, not a live derivation of it (same pattern as FileView.svelte).
	let watchedFor = current.file;
	$effect(() => {
		if (current.file !== watchedFor) {
			watchedFor = current.file;
			loadParentLine();
			watchPaths([current.file]);
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
		{#if popoverOpen}
			<EditPopover
				path={current.file}
				initialLine={parentLine}
				taskRef={parentTaskRef}
				anchor={null}
				onSave={saveParentEdit}
				onCancel={closePopover}
			/>
		{:else}
			<button type="button" class="parent-line" onclick={openPopover}>
				<code>{parentLine}</code>
			</button>
		{/if}
		{#if saveError}
			<p class="error" role="alert">{saveError}</p>
		{/if}
		{#if allSubTasksDone}
			<button type="button" class="mark-done-offer" onclick={markParentDone}>
				Mark parent done
			</button>
		{/if}
	</section>

	<section class="notes" aria-label="Notes">
		<h2>Notes</h2>
		{#if !parentLine}
			<!-- still loading -->
		{:else if !parentTaskId}
			<!-- `GetNotes`/`EditNotes` resolve a task by task_id alone
			     (crates/txtodo-daemon/src/notes.rs::locate_task), never by line_number — unlike
			     Edit/Complete/Delete, which resolve by line_number and treat an absent task_id as
			     harmless (crates/txtodo-daemon/src/mutation.rs::resolve). Under this workspace's
			     `identity_mode` (plan: sidecar is now the default, tasks/sidecar-identity/notes.md),
			     an existing task's id isn't written into the file, and the desktop has no RPC today
			     that resolves an arbitrary existing line to its task_id without one already visible
			     — so notes genuinely aren't reachable here yet, not a bug in this view. -->
			<p class="empty-state">
				Notes aren't available for this task yet: it has no id this app can resolve in the
				workspace's current identity mode. A task created with a hand-written
				<code>id:</code> tag (or in "tagged" mode) can use notes normally.
			</p>
		{:else}
			<NotesEditor task={parentTaskRef} />
		{/if}
	</section>

	<section class="sublist" aria-label="Sub-list">
		{#if !refDir}
			<p class="empty-state">
				No sub-list yet. Add a note above first — this task's own to-do list can only be
				created once it has a <code>ref:</code> directory (today that's minted by the first
				notes edit).
			</p>
		{:else if !subListInfo}
			<p class="empty-state">
				This task's folder exists but its own to-do list hasn't been created there yet.
			</p>
		{:else}
			<h2>{subListInfo.done} of {subListInfo.total} done</h2>
			<!-- No `onEditRequest`: the sub-list hosts its own popover locally (FileView's built-in
			     fallback), the same as the root view would if MainView didn't host one either. -->
			<FileView path={subListPath ?? ""} {depth} onDetailRequest={onNavigateInto} />
		{/if}
	</section>

	<footer class="detail-footer">
		<span class="dir">{absoluteRefDir}</span>
	</footer>
</div>

<style>
	.detail-view {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		padding: 2rem;
		font-family:
			Inter,
			Avenir,
			Helvetica,
			Arial,
			sans-serif;
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
		border: 1px solid #d1d5db;
		border-radius: 8px;
		padding: 0.75rem 1rem;
		background: #f9fafb;
	}

	.parent-line {
		background: transparent;
		border: none;
		text-align: left;
		cursor: pointer;
		font-size: 1rem;
		width: 100%;
	}

	.mark-done-offer {
		align-self: flex-start;
	}

	.empty-state {
		color: #6b7280;
		font-style: italic;
	}

	.error {
		color: #b91c1c;
	}

	.detail-footer {
		font-size: 0.8rem;
		color: #6b7280;
		border-top: 1px solid #e5e7eb;
		padding-top: 0.5rem;
	}
</style>
