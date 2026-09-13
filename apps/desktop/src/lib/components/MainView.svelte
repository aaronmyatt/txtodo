<script lang="ts">
	// The app's top-level screen (tasks/desktop-main-view): the daemon connectivity banner (moved
	// here from the old proof-of-pipeline +page.svelte) and the root `todo.txt` file view, which
	// edits directly (click a line, type — no popover; see FileView.svelte's own module doc).
	import { onMount } from "svelte";
	import { daemonStatus, onDaemonStatus, retryConnect, setMainPopoverDirty, type DaemonStatus } from "$lib/daemon";
	import type { DetailParams } from "$lib/types";
	import ConflictBanner from "./ConflictBanner.svelte";
	import DetailView from "./DetailView.svelte";
	import FileView from "./FileView.svelte";

	// Component contract says `<FileView path="todo.txt" depth={0}/>` explicitly; we take that at
	// face value rather than round-tripping through `list_files` just to confirm the obvious.
	const ROOT_PATH = "todo.txt";

	let status = $state<DaemonStatus>("connecting");

	// Detail-view navigation (tasks/desktop-detail-view, plan §3.2): a stack of open levels, empty
	// meaning "show the root file view" — the detail view replaces this screen's content, it never
	// overlays it (plan §3.3: "a page, not a modal"). `MainView` is the sole owner of this stack;
	// `DetailView`/`Breadcrumb` only ever report navigation intent upward.
	let detail = $state<DetailParams[]>([]);

	function openDetail(params: DetailParams) {
		detail = [...detail, params];
	}

	/** `0` = home/root; `n` = truncate to the first `n` levels (re-showing level `n - 1`). Matches
	 * `Breadcrumb`'s `onNavigate` contract exactly, so both it and the back button share this. */
	function navigateToLevel(stackLength: number) {
		detail = detail.slice(0, stackLength);
	}

	async function retry() {
		status = await retryConnect();
	}

	/** Wired to the root `FileView`'s `onDirtyChange` — see `$lib/daemon.ts::setMainPopoverDirty`'s
	 * doc comment. Best-effort: a failed update here only affects which window the hotkey focuses
	 * next, never whether an edit commits. */
	function handleDirtyChange(dirty: boolean) {
		setMainPopoverDirty(dirty).catch(() => {});
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

	<div class="top-nav">
		<h1>txtodo</h1>
		<a href="/devices">Devices &amp; agents</a>
	</div>

	{#if detail.length === 0}
		<ConflictBanner path={ROOT_PATH} />

		<FileView path={ROOT_PATH} depth={0} fill onDirtyChange={handleDirtyChange} onDetailRequest={openDetail} />
	{:else}
		<DetailView steps={detail} onNavigateInto={openDetail} onNavigateToLevel={navigateToLevel} />
	{/if}
</main>

<style>
	.top-nav {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		padding: 0.75rem 1rem 0;
	}

	.main-view {
		display: flex;
		flex-direction: column;
		height: 100vh;
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
		margin: 0.75rem 1rem 0;
	}

</style>
