<script lang="ts">
	// Universal (ADR 0025; rebuilt for task desktop-ui-revamp from the c2 mockup): every registered
	// workspace's root-list tasks in one page. A stat strip, group by priority / due / project /
	// context / workspace, workspace and context chips, and a show-done toggle, all over one
	// bounded `universalTasks(true)` fetch (the model is `$lib/universal/rows.ts`). Clicking a row
	// switches to its workspace and hands MainView a `PendingUniversalNav` so it opens on that line.
	import { onMount, tick } from "svelte";
	import {
		switchWorkspace,
		universalSetDone,
		universalTasks,
		universalUndo,
		workspaceLayout,
		workspaceRoot,
		type UniversalTask
	} from "$lib/daemon";
	import { showToast } from "$lib/stores/toast";
	import { searchQuery, searchStep, universalHits } from "$lib/stores/search";
	import { isActiveQuery, matchesQuery } from "$lib/todotxt/query";
	import { wasmReady } from "$lib/wasmCore";
	import { currentWorkspaceRoot, workspaceLayoutStore } from "$lib/stores/workspaces";
	import { DEFAULT_LAYOUT } from "$lib/todotxt/lineInfo";
	import type { DetailParams } from "$lib/types";
	import DetailPanel from "./DetailPanel.svelte";
	import { localToday } from "./editPopoverLogic";
	import { contextCounts, groupRows, stats, toRows, type GroupKind, type Row, matchLine, wantsDone } from "$lib/universal/rows";
	import UniversalFilters from "./UniversalFilters.svelte";
	import UniversalRow from "./UniversalRow.svelte";
	import UniversalStats from "./UniversalStats.svelte";

	let tasks = $state<UniversalTask[]>([]);
	let loading = $state(true);
	let error = $state("");
	let group = $state<GroupKind>("priority");
	let showDone = $state(false);
	let context = $state<string | null>(null);
	/** Workspace ids switched off by their chip; at least one always stays on. */
	let hidden = $state<Set<string>>(new Set());

	const today = localToday();
	const rows = $derived(toRows(tasks, today));
	const totals = $derived(stats(rows));
	/** Workspaces by label, in name order, with their open counts. */
	const workspaces = $derived.by(() => {
		const byId = new Map<string, { id: string; label: string; open: number }>();
		for (const r of rows) {
			const w = byId.get(r.task.workspace_id) ?? { id: r.task.workspace_id, label: r.ws, open: 0 };
			if (!r.task.done) w.open++;
			byId.set(w.id, w);
		}
		return [...byId.values()].sort((a, b) => a.label.localeCompare(b.label));
	});
	// The header search (shared with Tasks): the core's matcher over each row as a line; `is:done`
	// brings done rows in even with show-done off. The matcher needs the wasm core loaded.
	let wasmLoaded = $state(false);
	const query = $derived(wasmLoaded ? $searchQuery : "");
	const inScope = $derived(
		rows.filter((r) => !hidden.has(r.task.workspace_id) && (showDone || wantsDone(query) || !r.task.done))
	);
	const contexts = $derived(contextCounts(inScope));
	const shown = $derived(
		inScope.filter(
			(r) =>
				(!context || r.task.contexts.includes(context)) &&
				(!isActiveQuery(query) || matchesQuery(matchLine(r.task), query))
		)
	);
	$effect(() => universalHits.set(shown.length));
	const groups = $derived(groupRows(shown, group, workspaces.map((w) => w.label)));

	// Keyboard (roving tabindex, https://www.w3.org/WAI/ARIA/apg/practices/keyboard-interface/#kbd_roving_tabindex):
	// one row is the tab stop; j/k/arrows/Home/End move it, Enter opens (the button's own click),
	// x completes. `focusKey` survives a refetch, so the ring stays on the same task, or on the
	// row now at its place when that task left the list.
	const flat = $derived(groups.flatMap((g) => g.rows));
	let focusKey = $state<string | null>(null);
	const activeKey = $derived(flat.some((r) => r.key === focusKey) ? focusKey : (flat[0]?.key ?? null));
	let listEl: HTMLElement | undefined = $state();

	async function focusRow(i: number) {
		const r = flat[Math.max(0, Math.min(flat.length - 1, i))];
		if (!r) return;
		focusKey = r.key;
		await tick();
		listEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(r.key)}"]`)?.focus();
	}

	function onListKeydown(e: KeyboardEvent) {
		const key = (e.target as HTMLElement).closest<HTMLElement>("[data-key]")?.dataset.key;
		if (!key || e.metaKey || e.ctrlKey || e.altKey) return;
		const i = flat.findIndex((r) => r.key === key);
		const moves: Record<string, number> = { j: i + 1, ArrowDown: i + 1, k: i - 1, ArrowUp: i - 1, Home: 0, End: flat.length - 1 };
		if (e.key in moves) {
			e.preventDefault();
			void focusRow(moves[e.key]);
		} else if (e.key === "x" && flat[i]) {
			e.preventDefault();
			void toggle(flat[i], i);
		}
	}

	function toggleWorkspace(id: string) {
		const next = new Set(hidden);
		if (next.has(id)) next.delete(id);
		else if (workspaces.length - next.size > 1) next.add(id);
		hidden = next;
	}

	function resetFilters() {
		hidden = new Set();
		context = null;
		$searchQuery = "";
	}

	// The detail opens here, as the same bottom panel Tasks uses (2026-10-06 feedback: opening it
	// must not leave Universal). The detail reads through the app's open workspace, so a row from
	// another one switches to it first, with that workspace's layout for its ref: folders.
	let detail = $state<DetailParams[]>([]);
	let detailRoot = $state("todo.txt");

	async function open(task: UniversalTask) {
		error = "";
		try {
			if (task.workspace_root !== $currentWorkspaceRoot) {
				await switchWorkspace(task.workspace_root);
				$currentWorkspaceRoot = await workspaceRoot();
				workspaceLayoutStore.set(await workspaceLayout().catch(() => DEFAULT_LAYOUT));
			}
			detailRoot = task.root_list;
			detail = [{ file: task.root_list, line: task.line_number, workspaceRoot: task.workspace_root }];
		} catch (e) {
			error = String(e);
		}
	}

	/** `0` closes the panel (and refetches: the detail may have changed rows); `n` keeps n levels. */
	function navigateToLevel(stackLength: number) {
		detail = detail.slice(0, stackLength);
		if (stackLength === 0) void refresh();
	}

	/** One bounded fetch (done lines included, for the `x done` stat and show-done). */
	async function refresh() {
		try {
			tasks = await universalTasks(true);
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	}

	/** Completes or reopens a row in its own workspace, then offers Undo (the daemon's Undo on
	 * that file, so it reverses exactly this op unless something else wrote there meanwhile). */
	async function toggle(r: Row, index?: number) {
		error = "";
		const { task } = r;
		const hadFocus = index !== undefined;
		try {
			const { applied } = await universalSetDone(task, !task.done, localToday());
			await refresh();
			// A completed row may leave the list (show done off): keep the keyboard at its place.
			if (hadFocus) await focusRow(flat.some((x) => x.key === r.key) ? flat.findIndex((x) => x.key === r.key) : index);
			showToast(`${task.done ? "Reopened" : "Completed"} in ${r.ws}`, () => {
				void universalUndo(task.workspace_id, task.root_list, applied)
					.then(refresh)
					.catch((e) => (error = String(e)));
			});
		} catch (e) {
			error = String(e);
		}
	}

	// Enter in the header search hands the keyboard to the first matching row.
	let lastStep = $searchStep.n;
	$effect(() => {
		if ($searchStep.n === lastStep) return;
		lastStep = $searchStep.n;
		void focusRow(0);
	});

	onMount(() => {
		void refresh();
		void wasmReady().then(() => (wasmLoaded = true));
	});
</script>

<!-- Refetch when the window comes back: one bounded fetch, not a live tail. -->
<svelte:window onfocus={() => void refresh()} />

<div class="universal-page">
<main class="universal">
	<div class="page">
		<h1>Universal</h1>
		<p class="lede">Open tasks from every workspace. Click one to open its detail here.</p>
		<p class="keys"><kbd>j</kbd> <kbd>k</kbd> move <kbd>↵</kbd> open <kbd>x</kbd> complete</p>

		{#if error}
			<p class="error" role="alert">{error}</p>
		{/if}

		{#if loading}
			<p class="lede" role="status">Loading&hellip;</p>
		{:else}
			<UniversalStats {totals} />

			<UniversalFilters
				bind:group
				bind:showDone
				bind:context
				{workspaces}
				{hidden}
				{contexts}
				onToggleWorkspace={toggleWorkspace}
			/>

			{#if shown.length === 0}
				<div class="empty">
					<p><b>Nothing matches.</b></p>
					<p class="lede">Loosen the search or the filters above.</p>
					<button type="button" class="reset" onclick={resetFilters}>Reset filters</button>
				</div>
			{/if}

			<!-- svelte-ignore a11y_no_static_element_interactions: the keys belong to the row buttons inside; this only delegates. -->
			<div class="groups" bind:this={listEl} onkeydown={onListKeydown} onfocusin={(e) => (focusKey = (e.target as HTMLElement).dataset.key ?? focusKey)}>
				{#each groups as g (g.name)}
					<section aria-label={g.name}>
						<h2>
							<span
								class:tok-priority={group === "priority" && g.name.startsWith("(")}
								class:tok-project={group === "project" && g.name.startsWith("+")}
								class:tok-context={group === "context" && g.name.startsWith("@")}
								class:overdue={g.name === "Overdue"}
								class:soon={g.name === "Today"}
								class:muted={g.name.startsWith("No ")}>{g.name}</span>
							<span class="count">{g.rows.length}</span>
							<span class="rule"></span>
						</h2>
						{#each g.rows as r (r.key)}
							<UniversalRow row={r} {group} {query} onOpen={open} onToggle={toggle} active={r.key === activeKey} />
						{/each}
					</section>
				{/each}
			</div>
		{/if}
	</div>
</main>
{#if detail.length > 0}
	<DetailPanel steps={detail} rootPath={detailRoot} onNavigateInto={(p) => (detail = [...detail, p])} onNavigateToLevel={navigateToLevel} />
{/if}
</div>

<style>
	/* The page above, the detail panel under it when one is open. */
	.universal-page {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-height: 0;
	}

	.universal {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
	}

	.page {
		max-width: 64rem;
		margin: 0 auto;
		padding: 1.5rem 1.25rem 3rem;
	}

	h1 {
		margin: 0;
		font-size: 1.5rem;
		letter-spacing: -0.05em;
	}

	.lede {
		margin: 0.25rem 0 0;
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
		font-family: var(--font-sans);
		font-size: 0.875rem;
	}

	.keys {
		margin: 0.5rem 0 0;
		font-size: 11px;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.keys kbd {
		padding: 0 0.3rem;
		border: 1px solid currentColor;
		border-radius: 0.3rem;
		font: inherit;
	}

	.overdue {
		color: var(--color-error, #dc2626);
	}

	.soon {
		color: var(--color-warning, #d97706);
	}

	.muted {
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.empty {
		margin-top: 2.5rem;
		padding: 2rem;
		border: 2px dashed color-mix(in srgb, var(--color-base-content) 30%, transparent);
		border-radius: var(--radius-box);
		text-align: center;
	}

	.reset {
		padding: 0.25rem 0.9rem;
		border: 0;
		border-radius: 999px;
		background: var(--color-primary);
		color: var(--color-primary-content);
		font: inherit;
		font-size: 0.75rem;
		font-weight: 700;
		cursor: pointer;
	}

	.empty p {
		margin: 0 0 0.25rem;
	}

	.groups {
		display: flex;
		flex-direction: column;
		gap: 1.75rem;
		margin-top: 1.5rem;
	}

	h2 {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin: 0 0 0.5rem;
		font-size: 1rem;
	}

	.count {
		padding: 0 0.4rem;
		border-radius: 999px;
		background: var(--color-base-200);
		font-size: 0.75rem;
		font-weight: 400;
	}

	.rule {
		flex: 1;
		height: 2px;
		border-radius: 999px;
		background: color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	/* Group headings in the editor's token colours (todotxtLanguage.ts). */
	h2 .tok-priority {
		color: var(--tok-priority);
		font-size: 1.125rem;
	}

	h2 .tok-project {
		color: var(--tok-project);
	}

	h2 .tok-context {
		color: var(--tok-context);
	}

	.error {
		color: var(--color-error, crimson);
	}

</style>
