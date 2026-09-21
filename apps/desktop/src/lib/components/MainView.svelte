<script lang="ts">
	// The app's top-level screen (tasks/desktop-main-view): the daemon connectivity banner (moved
	// here from the old proof-of-pipeline +page.svelte) and the root `todo.txt` file view, which
	// edits directly (click a line, type — no popover; see FileView.svelte's own module doc).
	import { onMount } from "svelte";
	import { get } from "svelte/store";
	import {
		daemonStatus,
		onDaemonStatus,
		retryConnect,
		setMainPopoverDirty,
		workspaceLayout,
		workspaceRoot,
		type DaemonStatus
	} from "$lib/daemon";
	import { applyStoredPin } from "$lib/stores/pin";
	import { currentWorkspaceRoot, pendingUniversalNav, workspaceLayoutStore } from "$lib/stores/workspaces";
	import type { DetailParams } from "$lib/types";
	import ConflictBanner from "./ConflictBanner.svelte";
	import RejectedEditBanner from "./RejectedEditBanner.svelte";
	import { openingWorkspace } from "$lib/stores/loading";
	import DetailView from "./DetailView.svelte";
	import FileView from "./FileView.svelte";
	import PinToggle from "./PinToggle.svelte";
	import SkillHintBanner from "./SkillHintBanner.svelte";
	import VersionInfo from "./VersionInfo.svelte";
	import ThemeToggle from "./ThemeToggle.svelte";
	import WorkspaceSwitcher from "./WorkspaceSwitcher.svelte";

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
	let lastRoot = "";
	$effect(() => {
		const root = $currentWorkspaceRoot;
		if (!root || root === lastRoot) return;
		lastRoot = root;
		// Where this workspace keeps its `ref:` folders (task workspace-layout): the nested-list
		// paths and the ref indicators are composed from it. A failed fetch keeps the default.
		workspaceLayout()
			.then((layout) => workspaceLayoutStore.set(layout))
			.catch(() => {});
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

	<SkillHintBanner />

	<div class="top-nav">
		<div class="top-nav-left">
			<WorkspaceSwitcher />
			<h1>txtodo</h1>
		</div>
		<div class="top-nav-actions">
			<ThemeToggle />
			<PinToggle />
			<a href="/universal">Universal view</a>
			<a href="/devices">Devices &amp; agents</a>
			<a href="/help" class="help-link" aria-label="Help">?</a>
			<VersionInfo />
		</div>
	</div>

	<RejectedEditBanner />
	<VersionInfo banner />

	{#if $openingWorkspace > 0}
		<p class="opening" role="status">Opening this workspace&hellip;</p>
	{/if}

	{#if $currentWorkspaceRoot}
	{#key $currentWorkspaceRoot}
		{#if detail.length === 0}
			<ConflictBanner path={ROOT_PATH} />

			<FileView path={ROOT_PATH} depth={0} fill onDirtyChange={handleDirtyChange} onDetailRequest={openDetail} />
		{:else}
			<DetailView steps={detail} onNavigateInto={openDetail} onNavigateToLevel={navigateToLevel} />
		{/if}
	{/key}
	{/if}
</main>

<style>
	.opening {
		margin: 0.5rem 1rem;
		color: var(--color-text-muted, inherit);
		font-size: 0.85rem;
	}

	.top-nav {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		padding: 0.75rem 1rem 0;
	}

	.top-nav-left {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.top-nav-actions {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.help-link {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 1.5rem;
		height: 1.5rem;
		border: 1px solid var(--color-border);
		border-radius: 999px;
		text-decoration: none;
		color: inherit;
	}

	.main-view {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background: var(--color-bg);
		color: var(--color-text);
		font-family: var(--font-sans);
	}

	.top-nav h1 {
		margin: 0;
		font-family: var(--font-brand);
		font-size: 1.25rem;
		font-weight: 700;
		letter-spacing: -0.02em;
	}

	.banner {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		background: var(--color-banner-bg);
		color: var(--color-banner-text);
		padding: 0.5rem 1rem;
		border-radius: 6px;
		margin: 0.75rem 1rem 0;
	}

</style>
