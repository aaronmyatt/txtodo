<script lang="ts">
	// Workspace + Activity nav sidebar (task desktop-workspace-nav-sidebar, storyboard screens
	// 07/08): replaces the old anchored dropdown with a dismissable left-nav sidebar toggled from
	// MainView's titlebar. Same pick/add/remove/refresh daemon calls as before (ADR 0025, task
	// desktop-workspace-switcher) — only the shell moved; MainView still wraps its file view in
	// `{#key $currentWorkspaceRoot}` so a switch remounts FileView/ConflictBanner.
	//
	// Esc/outside-click dismiss and focus-on-open/restore-on-close follow
	// ConflictReviewSheet.svelte's own pattern (this codebase's existing "dismissable floating
	// panel" idiom) — its Tab focus-trap is not reused here since this sidebar is a `role="menu"`
	// disclosure, not a modal `role="dialog"`, so content behind it stays reachable while it's
	// open. Ref (disclosure pattern): https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/
	// Ref (tabs pattern): https://www.w3.org/WAI/ARIA/apg/patterns/tabs/
	//
	// The Activity tab renders ActivityTab.svelte (task desktop-activity-cross-workspace): a
	// separate component, not inlined here, to keep this file focused on the sidebar shell itself.
	import { tick } from "svelte";
	import { fly } from "svelte/transition";
	import {
		addWorkspace,
		listWorkspaces,
		removeWorkspace,
		switchWorkspace,
		workspaceRoot,
		type WorkspaceInfo
	} from "$lib/daemon";
	import { currentWorkspaceRoot } from "$lib/stores/workspaces";
	import ActivityTab from "./ActivityTab.svelte";

	let open = $state(false);
	let activeTab = $state<"workspaces" | "activity">("workspaces");
	let workspaces = $state<WorkspaceInfo[]>([]);
	let newPath = $state("");
	let error = $state("");
	let busy = $state(false);

	let toggleRef: HTMLButtonElement | undefined = $state();
	let asideRef: HTMLElement | undefined = $state();
	let previouslyFocused: HTMLElement | null = null;

	async function refresh() {
		workspaces = await listWorkspaces();
	}

	function focusableElements(): HTMLElement[] {
		if (!asideRef) return [];
		const selector = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';
		return Array.from(asideRef.querySelectorAll<HTMLElement>(selector)).filter(
			(el) => !el.hasAttribute("disabled")
		);
	}

	async function openSidebar() {
		if (open) return;
		previouslyFocused = document.activeElement as HTMLElement | null;
		open = true;
		error = "";
		try {
			await refresh();
		} catch (e) {
			error = String(e);
		}
		await tick(); // https://svelte.dev/docs/svelte/lifecycle-hooks#tick — wait for the aside to render
		focusableElements()[0]?.focus();
	}

	function closeSidebar() {
		if (!open) return;
		open = false;
		(previouslyFocused ?? toggleRef)?.focus();
		previouslyFocused = null;
	}

	function toggle() {
		if (open) closeSidebar();
		else openSidebar();
	}

	function onWindowKeydown(e: KeyboardEvent) {
		if (e.key === "Escape") closeSidebar();
	}

	// Outside-click dismiss: bound to `window` only while open (removed on close), checking the
	// event target against the sidebar's own node and the toggle button (a click on the toggle
	// itself already closes it through `toggle()` — this must not also fire and re-open it).
	function onWindowPointerdown(e: PointerEvent) {
		const target = e.target as Node | null;
		if (!target) return;
		if (asideRef?.contains(target) || toggleRef?.contains(target)) return;
		closeSidebar();
	}

	$effect(() => {
		if (!open) return;
		window.addEventListener("keydown", onWindowKeydown);
		window.addEventListener("pointerdown", onWindowPointerdown);
		return () => {
			window.removeEventListener("keydown", onWindowKeydown);
			window.removeEventListener("pointerdown", onWindowPointerdown);
		};
	});

	async function pick(root: string) {
		if (root === $currentWorkspaceRoot || busy) return;
		error = "";
		busy = true;
		try {
			await switchWorkspace(root);
			$currentWorkspaceRoot = await workspaceRoot();
			closeSidebar();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function add(e: SubmitEvent) {
		e.preventDefault();
		const root = newPath.trim();
		if (!root || busy) return;
		error = "";
		busy = true;
		try {
			await addWorkspace(root);
			newPath = "";
			await refresh();
		} catch (e2) {
			error = String(e2);
		} finally {
			busy = false;
		}
	}

	async function remove(ws: WorkspaceInfo) {
		if (busy) return;
		error = "";
		busy = true;
		try {
			await removeWorkspace(ws.id, ws.root);
			await refresh();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}
</script>

<button
	type="button"
	class="nav-toggle"
	bind:this={toggleRef}
	onclick={toggle}
	aria-expanded={open}
	aria-label={open ? "Close workspace navigation" : "Open workspace navigation"}
>
	<span aria-hidden="true">▤</span>
</button>

{#if open}
	<!-- A plain div, not <aside>: svelte-check's a11y rule flags a landmark element (aside) being
	     given an interactive role like "menu" — same fix ConflictReviewSheet.svelte uses for its
	     own dialog role. -->
	<div class="nav-sidebar" role="menu" bind:this={asideRef} transition:fly={{ x: -280, duration: 180 }}>
		<div class="tabs" role="tablist" aria-label="Workspace navigation">
			<button
				type="button"
				role="tab"
				id="nav-tab-workspaces"
				aria-selected={activeTab === "workspaces"}
				aria-controls="nav-panel-workspaces"
				class:active={activeTab === "workspaces"}
				onclick={() => (activeTab = "workspaces")}
			>
				Workspaces
			</button>
			<button
				type="button"
				role="tab"
				id="nav-tab-activity"
				aria-selected={activeTab === "activity"}
				aria-controls="nav-panel-activity"
				class:active={activeTab === "activity"}
				onclick={() => (activeTab = "activity")}
			>
				Activity
			</button>
		</div>

		{#if activeTab === "workspaces"}
			<div id="nav-panel-workspaces" class="tab-panel" role="tabpanel" aria-labelledby="nav-tab-workspaces">
				{#if error}
					<p class="error" role="alert">{error}</p>
				{/if}
				<ul>
					{#each workspaces as ws (ws.id)}
						<li class:current={ws.root === $currentWorkspaceRoot} class:missing={!ws.root_exists}>
							<button
								type="button"
								class="entry"
								onclick={() => pick(ws.root)}
								disabled={busy}
								title={ws.root_exists ? undefined : "This workspace's directory no longer exists on disk"}
							>
								{ws.root}
								{#if !ws.root_exists}<span class="missing-label">missing</span>{/if}
							</button>
							{#if ws.root !== $currentWorkspaceRoot}
								<button
									type="button"
									class="remove"
									aria-label={`Remove ${ws.root}`}
									onclick={() => remove(ws)}
									disabled={busy}
								>
									&times;
								</button>
							{/if}
						</li>
					{/each}
				</ul>
				<form onsubmit={add}>
					<input
						type="text"
						placeholder="/path/to/workspace"
						bind:value={newPath}
						disabled={busy}
					/>
					<button type="submit" disabled={busy}>Add</button>
				</form>
			</div>
		{:else}
			<div id="nav-panel-activity" class="tab-panel" role="tabpanel" aria-labelledby="nav-tab-activity">
				<ActivityTab />
			</div>
		{/if}
	</div>
{/if}

<style>
	.nav-toggle {
		background: transparent;
		border: 1px solid var(--color-border);
		border-radius: 6px;
		padding: 0.25rem 0.5rem;
		color: inherit;
		cursor: pointer;
		font-size: 1rem;
		line-height: 1;
	}

	.nav-sidebar {
		position: fixed;
		inset: 0 auto 0 0;
		z-index: 20;
		width: min(24rem, 85vw);
		display: flex;
		flex-direction: column;
		background: var(--color-bg-elevated);
		color: var(--color-text);
		border-right: 1px solid var(--color-border);
		box-shadow: 4px 0 16px rgba(0, 0, 0, 0.2);
		padding: 0.5rem;
	}

	.tabs {
		display: flex;
		gap: 0.25rem;
		border-bottom: 1px solid var(--color-border-subtle);
		margin-bottom: 0.5rem;
	}

	.tabs button {
		background: transparent;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 0.5rem 0.75rem;
		border-bottom: 2px solid transparent;
	}

	.tabs button.active {
		color: var(--color-text);
		border-bottom-color: var(--color-text);
		font-weight: 600;
	}

	.tab-panel {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
	}

	ul {
		list-style: none;
		margin: 0 0 0.5rem;
		padding: 0;
		flex: 1;
		overflow-y: auto;
	}

	li {
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.entry {
		flex: 1;
		text-align: left;
		background: transparent;
		border: none;
		padding: 0.35rem 0.4rem;
		color: inherit;
		cursor: pointer;
		border-radius: 4px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	li.current .entry {
		font-weight: 600;
	}

	/* tasks/test-registry-leak-cleanup: a dead (deleted or never-real) temp-dir registration
	   stays greyed out and labeled rather than hidden, matching `txtodo workspace list`'s own
	   `[missing]` annotation — still pickable/removable, just visually deprioritized so leaked
	   entries don't read as equally-live choices. */
	li.missing .entry {
		opacity: 0.5;
	}

	.missing-label {
		margin-left: 0.4em;
		font-size: 0.75em;
		opacity: 0.8;
	}

	.entry:hover:not(:disabled) {
		background: var(--color-hover-overlay);
	}

	.remove {
		background: transparent;
		border: none;
		color: inherit;
		opacity: 0.6;
		cursor: pointer;
		padding: 0.2rem 0.45rem;
	}

	.remove:hover:not(:disabled) {
		opacity: 1;
	}

	form {
		display: flex;
		gap: 0.35rem;
	}

	form input {
		flex: 1;
		min-width: 0;
	}

	.error {
		margin: 0 0 0.5rem;
		color: var(--color-danger);
	}
</style>
