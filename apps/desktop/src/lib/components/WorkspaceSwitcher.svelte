<script lang="ts">
	// The workspace menu (task desktop-ui-revamp, the c2 mockup's header dropdown; was a left-nav
	// sidebar, task desktop-workspace-nav-sidebar): a pill in the AppShell header naming the open
	// workspace, opening a panel under it with each workspace's open count, the current one
	// checked, missing roots greyed, and a "Manage workspaces" link to Settings. Same
	// pick/add/remove/rename/refresh daemon calls as before (ADR 0025); MainView still wraps its
	// file view in `{#key $currentWorkspaceRoot}` so a switch remounts FileView/ConflictBanner.
	// Activity lives in Settings > Activity now (it was this menu's second tab).
	//
	// Esc/outside-click dismiss and focus-on-open/restore-on-close follow
	// ConflictReviewSheet.svelte's own pattern (this codebase's existing "dismissable floating
	// panel" idiom) — its Tab focus-trap is not reused here since this sidebar is a `role="menu"`
	// disclosure, not a modal `role="dialog"`, so content behind it stays reachable while it's
	// open. Ref (disclosure pattern): https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/
	import { onMount, tick } from "svelte";
	import { onWorkspaceMenuRequest } from "$lib/workspaceMenu";
	import {
		addWorkspace,
		listWorkspaces,
		newList,
		removeList,
		removeWorkspace,
		renameList,
		renameWorkspace,
		switchWorkspace,
		universalTasks,
		workspaceRoot,
		type WorkspaceInfo
	} from "$lib/daemon";
	import Icon from "$lib/icons/Icon.svelte";
	import { currentWorkspaceRoot } from "$lib/stores/workspaces";
	import { folderOf, groupWorkspaces } from "$lib/workspaceGroups";

	let open = $state(false);
	let workspaces = $state<WorkspaceInfo[]>([]);
	let newPath = $state("");
	// The name of a list being made (task two-kinds-of-workspace).
	let newListName = $state("");
	let error = $state("");
	let busy = $state(false);
	// The workspace whose name is being edited, and the text so far (task workspace-vanity-name).
	let renaming = $state<string | null>(null);
	let newName = $state("");
	let renameRef: HTMLInputElement | undefined = $state();

	let toggleRef: HTMLButtonElement | undefined = $state();
	let asideRef: HTMLElement | undefined = $state();
	let previouslyFocused: HTMLElement | null = null;

	// Open tasks per workspace root, from the one cross-workspace list the daemon already serves
	// (`universalTasks`, open lines of each root list); empty when that fetch fails.
	let openCounts = $state<Map<string, number>>(new Map());

	// The counts only show in the open menu, so they are fetched only then: at startup the fetch
	// reads every workspace's root list and cost the 10k-line first paint ~500ms (e2e/perf.spec.ts).
	async function refresh() {
		workspaces = await listWorkspaces();
		if (!open) return;
		universalTasks()
			.then((tasks) => {
				const counts = new Map<string, number>();
				for (const t of tasks) counts.set(t.workspace_root, (counts.get(t.workspace_root) ?? 0) + 1);
				openCounts = counts;
			})
			.catch(() => (openCounts = new Map()));
	}

	// Lists first and repos after, each under a heading (`workspaceGroups.ts`).
	const groups = $derived(groupWorkspaces(workspaces));

	/** What the trigger and a row call a workspace: its name when it has one, else its folder. */
	function title(ws: WorkspaceInfo): string {
		return shownName(ws) || (ws.is_default ? "Default" : folderOf(ws.root));
	}

	const currentTitle = $derived.by(() => {
		const ws = workspaces.find((w) => w.root === $currentWorkspaceRoot);
		return ws ? title(ws) : folderOf($currentWorkspaceRoot ?? "") || "Workspace";
	});
	// Fill the trigger's name as soon as the open workspace is known, not only once the menu opens.
	$effect(() => {
		if ($currentWorkspaceRoot) refresh().catch(() => {});
	});

	// The name every paired device shows (task workspace-vanity-name), beside the path only when it
	// says more than the folder does: a mirror's folder is named by the workspace id.
	function shownName(ws: WorkspaceInfo): string {
		const folder = ws.root.split(/[\\/]/).filter(Boolean).pop() ?? "";
		const plain = !ws.name || ws.name === folder || (ws.is_default && ws.name === "default");
		return plain ? "" : (ws.name ?? "");
	}

	// While the daemon is still opening workspaces (task daemon-early-bind) keep the list fresh so
	// "opening…" turns into a plain entry by itself; stops as soon as nothing is pending or the
	// sidebar closes.
	const pending = $derived(workspaces.some((w) => w.load_state === "queued" || w.load_state === "loading"));
	$effect(() => {
		if (!open || !pending) return;
		const timer = setInterval(() => {
			refresh().catch(() => {});
		}, 2000);
		return () => clearInterval(timer);
	});

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

	/** The TUI's menu keys on a focused row (workspace_menu.*): j/k or the arrows move, Enter
	 * opens (the row's own click), W closes like Esc. A rename field keeps its letters. */
	function onMenuKeydown(e: KeyboardEvent) {
		const target = e.target as HTMLElement;
		if (!target.classList.contains("entry")) return;
		const rows = [...(asideRef?.querySelectorAll<HTMLElement>("button.entry:not([disabled])") ?? [])];
		const i = rows.indexOf(target);
		const go = (n: number) => rows[Math.max(0, Math.min(rows.length - 1, n))]?.focus();
		if (e.key === "j" || e.key === "ArrowDown") go(i + 1);
		else if (e.key === "k" || e.key === "ArrowUp") go(i - 1);
		else if (e.key === "W") closeSidebar();
		else return;
		e.preventDefault();
	}

	// `W` from anywhere (`$lib/workspaceMenu.ts`) opens it, and closes it when open; opening puts
	// the keyboard on the open workspace's row.
	onMount(() =>
		onWorkspaceMenuRequest(() => {
			if (open) return closeSidebar();
			void openSidebar().then(() => {
				const row =
					asideRef?.querySelector<HTMLElement>("li.current button.entry") ??
					asideRef?.querySelector<HTMLElement>("button.entry");
				row?.focus();
			});
		})
	);

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

	/** Makes the list typed in the New list field; the daemon checks the name. */
	async function makeList(e: SubmitEvent) {
		e.preventDefault();
		const name = newListName.trim();
		if (!name || busy) return;
		error = "";
		busy = true;
		try {
			await newList(name);
			newListName = "";
			await refresh();
		} catch (e2) {
			error = String(e2);
		} finally {
			busy = false;
		}
	}

	async function startRename(ws: WorkspaceInfo) {
		error = "";
		renaming = ws.id;
		newName = ws.name ?? "";
		await tick(); // https://svelte.dev/docs/svelte/lifecycle-hooks#tick — wait for the input
		renameRef?.select();
	}

	// Esc cancels the rename only. Stopping it here keeps the window's Esc handler from closing
	// the whole sidebar. Ref: https://developer.mozilla.org/en-US/docs/Web/API/Event/stopPropagation
	function onRenameKeydown(e: KeyboardEvent) {
		if (e.key !== "Escape") return;
		e.stopPropagation();
		renaming = null;
	}

	async function rename(e: SubmitEvent, ws: WorkspaceInfo) {
		e.preventDefault();
		if (busy) return;
		const name = newName.trim();
		// Unchanged: nothing is sent, so a workspace with no txtodo.toml does not get one for
		// nothing. Empty is sent: it clears the name.
		if (name === (ws.name ?? "")) {
			renaming = null;
			return;
		}
		error = "";
		busy = true;
		try {
			// A list is renamed through `ListRename`: the daemon checks the name as a list name and
			// finds it free. It is addressed by its folder, the name it was made under.
			if (ws.kind === "list") await renameList(folderOf(ws.root), name);
			else await renameWorkspace(ws.id, name);
			renaming = null;
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
			// A list moves its folder aside and stays on the other devices; any other workspace is
			// only forgotten, its files untouched.
			if (ws.kind === "list") await removeList(folderOf(ws.root), ws.root);
			else await removeWorkspace(ws.id, ws.root);
			await refresh();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}
</script>

<div class="ws-switcher">
<button
	type="button"
	class="ws-trigger"
	bind:this={toggleRef}
	onclick={toggle}
	aria-expanded={open}
	aria-label={open ? "Close workspace navigation" : "Open workspace navigation"}
	title={$currentWorkspaceRoot ?? undefined}
>
	<span class="ws-name">{currentTitle}</span>
	<Icon name="down" />
</button>

{#if open}
	<!-- A plain div, not <aside>: svelte-check's a11y rule flags a landmark element (aside) being
	     given an interactive role like "menu" — same fix ConflictReviewSheet.svelte uses for its
	     own dialog role. -->
	<div class="ws-menu" role="menu" tabindex="-1" bind:this={asideRef} onkeydown={onMenuKeydown}>
		<div id="nav-panel-workspaces" class="tab-panel">
			{#if error}
				<p class="error" role="alert">{error}</p>
			{/if}
			<ul>
				{#each groups as group (group.heading ?? "")}
				{#if group.heading}<li class="group-head" role="presentation">{group.heading}</li>{/if}
				{#each group.items as ws (ws.id)}
					<li class:current={ws.root === $currentWorkspaceRoot} class:missing={!ws.root_exists}>
						{#if renaming === ws.id}
							<form class="rename-form" onsubmit={(e) => rename(e, ws)}>
								<input
									type="text"
									aria-label={`Name for ${ws.root}`}
									placeholder="empty shows the folder's name"
									bind:this={renameRef}
									bind:value={newName}
									onkeydown={onRenameKeydown}
									disabled={busy}
								/>
								<button type="submit" disabled={busy}>Save</button>
							</form>
						{:else}
							<button
								type="button"
								class="entry"
								onclick={() => pick(ws.root)}
								disabled={busy}
								title={ws.root_exists ? undefined : "This workspace's directory no longer exists on disk"}
							>
								<span class="mark-slot" aria-hidden="true">
									{#if ws.root === $currentWorkspaceRoot}<Icon name="check" />{/if}
								</span>
								<span class="entry-text">
									<span class="entry-title">
										{title(ws)}
										{#if ws.is_remote}<span class="tag" title="Synced from another of your devices; txtodo keeps it in its own data folder">Remote</span>{/if}
									</span>
									<span class="entry-path">{ws.root}</span>
								</span>
								{#if openCounts.has(ws.root)}
									<span class="count" title="Open tasks">t {openCounts.get(ws.root)}</span>
								{/if}
								{#if !ws.root_exists}<span class="missing-label">missing</span>{/if}
								{#if ws.load_state === "queued" || ws.load_state === "loading"}
									<span class="load-label" role="status">{ws.load_state === "loading" ? "opening…" : "waiting to open"}</span>
								{:else if ws.load_state === "failed"}
									<span class="load-label failed" title={ws.load_error}>failed to open</span>
								{/if}
							</button>
							<button
								type="button"
								class="remove"
								aria-label={`Rename ${ws.root}`}
								title="Rename: the name every paired device shows"
								onclick={() => startRename(ws)}
								disabled={busy}
							>
								&#9998;
							</button>
							{#if ws.root !== $currentWorkspaceRoot && !ws.is_default}
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
						{/if}
					</li>
				{/each}
				{/each}
			</ul>
			<form class="new-list" onsubmit={makeList}>
				<input
					type="text"
					aria-label="New list name"
					placeholder="new list name"
					bind:value={newListName}
					disabled={busy}
				/>
				<button type="submit" disabled={busy}>New list</button>
			</form>
			<form onsubmit={add}>
				<input
					type="text"
					placeholder="/path/to/workspace"
					bind:value={newPath}
					disabled={busy}
				/>
				<button type="submit" disabled={busy}>Add</button>
			</form>
			<a class="manage" href="/settings#workspaces" onclick={closeSidebar}>Manage workspaces</a>
		</div>
	</div>
{/if}
</div>

<style>
	.ws-switcher {
		position: relative;
		flex-shrink: 0;
	}

	.ws-trigger {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		max-width: 14rem;
		padding: 0.25rem 0.5rem;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font: inherit;
		cursor: pointer;
		--icon-size: 0.875rem;
	}

	.ws-trigger:hover,
	.ws-trigger[aria-expanded="true"] {
		background: var(--color-base-200);
	}

	.ws-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	/* The mockup's dropdown: a 2px ink outline, rounded box, under the trigger. */
	.ws-menu {
		position: absolute;
		top: calc(100% + 0.5rem);
		left: 0;
		z-index: 30;
		display: flex;
		flex-direction: column;
		width: min(24rem, 90vw);
		max-height: 70vh;
		padding: 0.5rem;
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
		font-size: 0.75rem;
	}

	.manage {
		margin-top: 0.5rem;
		padding: 0.35rem 0.4rem;
		color: inherit;
		font-weight: 700;
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
		display: flex;
		flex: 1;
		align-items: center;
		gap: 0.5rem;
		min-width: 0;
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

	.mark-slot {
		display: inline-flex;
		flex: none;
		width: 1rem;
		--icon-size: 1rem;
	}

	.entry-text {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-width: 0;
	}

	.entry-title {
		font-weight: 700;
	}

	.entry-path {
		overflow: hidden;
		text-overflow: ellipsis;
		opacity: 0.6;
	}

	/* The Lists / Repos headings (task two-kinds-of-workspace): quiet labels, not rows. */
	.group-head {
		margin: 0.4rem 0 0.15rem;
		padding: 0 0.4rem;
		font-size: 0.65rem;
		font-weight: 700;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		opacity: 0.6;
	}

	.new-list {
		margin-top: 0.5rem;
	}

	.tag {
		margin-left: 0.35rem;
		font-weight: 400;
		opacity: 0.7;
	}

	.count {
		flex: none;
		padding: 0 0.45rem;
		border-radius: 999px;
		background: var(--color-base-content);
		color: var(--color-base-100);
		font-weight: 700;
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

	.load-label {
		margin-left: 0.4em;
		font-size: 0.75em;
		opacity: 0.8;
	}

	.load-label.failed {
		color: var(--color-danger);
		opacity: 1;
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

	.rename-form {
		flex: 1;
		min-width: 0;
	}

	.error {
		margin: 0 0 0.5rem;
		color: var(--color-danger);
	}
</style>
