<script lang="ts">
	// The app's top-level screen (tasks/desktop-main-view): the daemon connectivity banner (moved
	// here from the old proof-of-pipeline +page.svelte), the root `todo.txt` file view, and the
	// edit popover host. `FileView` reports a click-to-edit via its optional `onEditRequest` prop;
	// hosting the popover here (rather than inside `FileView`) is what the task notes mean by
	// "MainView... is where you mount the edit popover once a line is clicked."
	import { onMount } from "svelte";
	import { applyMutations, daemonStatus, onDaemonStatus, retryConnect, type DaemonStatus } from "$lib/daemon";
	import type { EditRequest } from "$lib/todotxt/editRequest";
	import EditPopover from "./EditPopover.svelte";
	import FileView from "./FileView.svelte";

	// Component contract says `<FileView path="todo.txt" depth={0}/>` explicitly; we take that at
	// face value rather than round-tripping through `list_files` just to confirm the obvious.
	const ROOT_PATH = "todo.txt";

	let status = $state<DaemonStatus>("connecting");
	let popover = $state<EditRequest | null>(null);
	let saveError = $state("");

	async function retry() {
		status = await retryConnect();
	}

	async function savePopover(newLine: string) {
		if (!popover) return;
		const { path, taskRef } = popover;
		popover = null;
		saveError = "";
		try {
			await applyMutations(path, [{ kind: "edit", task: taskRef, new_line: newLine }]);
		} catch (e) {
			saveError = String(e);
		}
	}

	function cancelPopover() {
		popover = null;
	}

	onMount(() => {
		daemonStatus().then((s) => (status = s));
		const unlisten = onDaemonStatus((s) => {
			status = s;
		});
		return () => {
			unlisten.then((f) => f());
		};
	});
</script>

<main class="main-view">
	{#if status !== "connected"}
		<div class="banner" role="alert">
			<span>Daemon: {status}</span>
			<button onclick={retry}>Retry</button>
		</div>
	{/if}

	{#if saveError}
		<div class="banner error" role="alert">
			<span>Save failed: {saveError}</span>
		</div>
	{/if}

	<h1>txtodo</h1>

	<FileView path={ROOT_PATH} depth={0} onEditRequest={(req) => (popover = req)} />

	{#if popover}
		<EditPopover
			initialLine={popover.initialLine}
			taskRef={popover.taskRef}
			anchor={popover.anchor}
			onSave={savePopover}
			onCancel={cancelPopover}
		/>
	{/if}
</main>

<style>
	.main-view {
		padding: 2rem;
		font-family:
			Inter,
			Avenir,
			Helvetica,
			Arial,
			sans-serif;
	}

	.banner {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		background: #fde68a;
		color: #1f2937;
		padding: 0.5rem 1rem;
		border-radius: 6px;
		margin-bottom: 1rem;
	}

	.banner.error {
		background: #fecaca;
	}
</style>
