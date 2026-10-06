<script lang="ts">
	// The app's top-level screen (tasks/desktop-main-view): the daemon connectivity banner (moved
	// here from the old proof-of-pipeline +page.svelte) and the root `todo.txt` file view, which
	// edits directly (click a line, type — no popover; see FileView.svelte's own module doc).
	import { onMount } from "svelte";
	import { get } from "svelte/store";
	import { daemonStatus, onDaemonStatus, retryConnect, setMainPopoverDirty, workspaceLayout, workspaceRoot, type DaemonStatus, onDaemonChange } from "$lib/daemon";
	import { applyStoredPin } from "$lib/stores/pin";
	import { DEFAULT_LAYOUT } from "$lib/todotxt/lineInfo";
	import { currentWorkspaceRoot, pendingUniversalNav, workspaceLayoutStore, workspaceLayoutError } from "$lib/stores/workspaces";
	import type { DetailParams } from "$lib/types";
	import ConflictBanner from "./ConflictBanner.svelte";
	import { editorStatus } from "$lib/stores/editorStatus";
	import { searchQuery } from "$lib/stores/search";
	import ProtocolMismatchBanner from "./ProtocolMismatchBanner.svelte";
	import RejectedEditBanner from "./RejectedEditBanner.svelte";
	import { openingWorkspace } from "$lib/stores/loading";
	import DetailView from "./DetailView.svelte";
	import FileView from "./FileView.svelte";
	import SkillHintBanner from "./SkillHintBanner.svelte";
	import VersionInfo from "./VersionInfo.svelte";
	import { layoutFallbackMessage } from "$lib/layoutFallback";
	import { LAYOUT_CHANGE_PATH } from "$lib/todotxt/lineInfo";

	// The root list is `todo_file` from the workspace's layout (task workspace-layout), `todo.txt`
	// until the answer arrives; nested lists stay `todo.txt`.
	const ROOT_PATH = $derived($workspaceLayoutStore.todo_file);

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

	// Detail levels are pinned to whichever workspace was open when they were pushed (a `{file,
	// line}` pair meaningless in another workspace's file tree) — drop the stack on every switch
	// so `{#key}` below never remounts `DetailView` with steps that don't resolve in the new root.
	// The one exception is a switch the universal view itself just triggered (WorkspaceSwitcher's
	// own `pick()` never sets `pendingUniversalNav`): then this effect opens exactly the level the
	// universal view asked for instead of dropping back to root, tagged with `workspaceRoot` so
	// `Breadcrumb` shows the owning project. `get()` (not `$pendingUniversalNav`) is deliberate: a
	// one-shot read on the root change that triggered it, not a second reactive dependency that
	// would re-run this effect on every unrelated store write.
	// With nothing picked the app opens the default workspace (task default-workspace), so there is
	// always a root: the main view stays empty only until the first `workspaceRoot()` answer.
	// Bumped at the start of every layout fetch (task layout-hot-reload-clients): a reply whose
	// number is no longer the latest lost to a newer fetch (a second workspace pick in quick
	// succession) and must not win — the same idiom as FileView's `refreshSeq`.
	let layoutSeq = 0;
	/** Where this workspace keeps its `ref:` folders and what its root list is called (task
	 * workspace-layout): the nested-list paths and the ref indicators are composed from it. A
	 * failed fetch keeps the default but says so (task desktop-notes-hidden): an older daemon
	 * without the RPC resolves ref folders elsewhere than this client. */
	async function fetchLayout(): Promise<void> {
		const seq = ++layoutSeq;
		try {
			const layout = await workspaceLayout();
			if (seq !== layoutSeq) return;
			workspaceLayoutStore.set(layout);
			workspaceLayoutError.set("");
		} catch (e) {
			if (seq !== layoutSeq) return;
			workspaceLayoutError.set(String(e));
		}
	}

	let lastRoot = "";
	$effect(() => {
		const root = $currentWorkspaceRoot;
		if (!root || root === lastRoot) return;
		lastRoot = root;
		// Reset first, so the new workspace never opens the previous one's root list.
		workspaceLayoutStore.set(DEFAULT_LAYOUT);
		workspaceLayoutError.set("");
		void fetchLayout();
		const pending = get(pendingUniversalNav);
		if (pending && pending.workspaceRoot === root) {
			detail = [{ file: pending.file, line: pending.line, workspaceRoot: pending.workspaceRoot }];
			pendingUniversalNav.set(null);
		} else {
			detail = [];
		}
	});

	onMount(() => {
		daemonStatus().then((s) => (status = s));
		workspaceRoot().then((r) => ($currentWorkspaceRoot = r));
		// A freshly created OS-level window always starts un-pinned; re-apply whatever was stored
		// from a previous session (task desktop-always-on).
		void applyStoredPin();
		const unlisten = onDaemonStatus((s) => {
			status = s;
		});
		// The daemon announces a layout change (the RPC, or a hot reload of `txtodo.toml`) as a
		// change for that file on the same stream every document change rides (task
		// layout-hot-reload-clients); the store is refetched, never patched from the event.
		const unlistenLayout = onDaemonChange((change) => {
			if (change.path === LAYOUT_CHANGE_PATH) void fetchLayout();
		});
		return () => {
			unlisten.then((f) => f());
			unlistenLayout.then((f) => f());
		};
	});
</script>

<main class="main-view">
	{#if status !== "connected"}
		<div class="banner tx-strip tx-strip-inverse" role="alert">
			<span>Daemon: {status}</span>
			<button onclick={retry}>Retry</button>
		</div>
	{/if}

	<ProtocolMismatchBanner />
	<SkillHintBanner />

	<RejectedEditBanner />
	<VersionInfo banner />
	{#if $workspaceLayoutError}
		<p class="layout-fallback" role="status">{layoutFallbackMessage($workspaceLayoutError)}</p>
	{/if}

	{#if $openingWorkspace > 0}
		<p class="opening" role="status">Opening this workspace&hellip;</p>
	{/if}

	{#if $currentWorkspaceRoot}
	{#key `${$currentWorkspaceRoot}|${ROOT_PATH}`}
		{#if detail.length === 0}
			<ConflictBanner path={ROOT_PATH} />

			<!-- Sub-toolbar: which list, and its counts (the logo's letters: t open, x done). -->
			<div class="subbar">
				<span class="file">{ROOT_PATH}</span>
				<span class="counts">
					<span>t {$editorStatus.counts.open}</span>
					<span>x {$editorStatus.counts.done}</span>
					<span>{$editorStatus.counts.lines} lines</span>
				</span>
				{#if $searchQuery.trim() !== ""}
					<span class="match">
						{$editorStatus.search.hits} of {$editorStatus.counts.lines} lines match
					</span>
				{/if}
			</div>
			<FileView path={ROOT_PATH} depth={0} fill reportStatus onDirtyChange={handleDirtyChange} onDetailRequest={openDetail} />
		{:else}
			<DetailView steps={detail} onNavigateInto={openDetail} onNavigateToLevel={navigateToLevel} />
		{/if}
	{/key}
	{/if}
</main>

<style>
	/* The mockup's sub-toolbar: h-8, 11px, a 10% ink rule under it. */
	.subbar {
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 0.75rem;
		height: 2rem;
		padding: 0 1rem;
		border-bottom: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
		font-family: var(--font-mono);
		font-size: 0.6875rem;
	}

	.file {
		font-weight: 700;
	}

	.counts {
		display: flex;
		gap: 0.75rem;
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
	}

	.match {
		margin-left: auto;
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
	}

	.opening {
		margin: 0.5rem 1rem;
		color: var(--color-text-muted, inherit);
		font-size: 0.85rem;
	}





	.main-view {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-height: 0;
		background: var(--color-bg);
		color: var(--color-text);
		font-family: var(--font-sans);
	}


	.layout-fallback {
		margin: 0.5rem 1rem;
		padding: 0.4rem 0.6rem;
		border: 1px solid var(--color-warning, #b45309);
		border-radius: 4px;
		font-size: 0.85rem;
	}


</style>
