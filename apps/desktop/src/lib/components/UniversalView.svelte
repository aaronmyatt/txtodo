<script lang="ts">
	// Universal (ADR 0025; rebuilt for task desktop-ui-revamp from the c2 mockup): every registered
	// workspace's root-list tasks in one page. A stat strip, group by priority / due / project /
	// context / workspace, workspace and context chips, and a show-done toggle, all over one
	// bounded `universalTasks(true)` fetch (the model is `$lib/universal/rows.ts`). Clicking a row
	// switches to its workspace and hands MainView a `PendingUniversalNav` so it opens on that line.
	import { goto } from "$app/navigation";
	import { onMount } from "svelte";
	import { switchWorkspace, universalSetDone, universalTasks, universalUndo, type UniversalTask } from "$lib/daemon";
	import { showToast } from "$lib/stores/toast";
	import { pendingUniversalNav } from "$lib/stores/workspaces";
	import { localToday } from "./editPopoverLogic";
	import { contextCounts, GROUP_KINDS, groupRows, stats, toRows, type GroupKind, type Row } from "$lib/universal/rows";
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
	const inScope = $derived(rows.filter((r) => !hidden.has(r.task.workspace_id) && (showDone || !r.task.done)));
	const contexts = $derived(contextCounts(inScope));
	const shown = $derived(context ? inScope.filter((r) => r.task.contexts.includes(context!)) : inScope);
	const groups = $derived(groupRows(shown, group, workspaces.map((w) => w.label)));

	function toggleWorkspace(id: string) {
		const next = new Set(hidden);
		if (next.has(id)) next.delete(id);
		else if (workspaces.length - next.size > 1) next.add(id);
		hidden = next;
	}

	function resetFilters() {
		hidden = new Set();
		context = null;
	}

	async function open(task: UniversalTask) {
		error = "";
		try {
			await switchWorkspace(task.workspace_root);
			pendingUniversalNav.set({ file: task.root_list, line: task.line_number, workspaceRoot: task.workspace_root });
			await goto("/");
		} catch (e) {
			error = String(e);
		}
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
	async function toggle(r: Row) {
		error = "";
		const { task } = r;
		try {
			await universalSetDone(task, !task.done, localToday());
			await refresh();
			showToast(`${task.done ? "Reopened" : "Completed"} in ${r.ws}`, () => {
				void universalUndo(task.workspace_id, task.root_list)
					.then(refresh)
					.catch((e) => (error = String(e)));
			});
		} catch (e) {
			error = String(e);
		}
	}

	onMount(refresh);
</script>

<main class="universal">
	<div class="page">
		<h1>Universal</h1>
		<p class="lede">Open tasks from every workspace. Click one to open it where it lives.</p>

		{#if error}
			<p class="error" role="alert">{error}</p>
		{/if}

		{#if loading}
			<p class="lede" role="status">Loading&hellip;</p>
		{:else}
			<UniversalStats {totals} />

			<div class="controls">
				<div class="caps group-by">
					Group
					<div class="join" role="group" aria-label="Group by">
						{#each GROUP_KINDS as g (g.kind)}
							<button type="button" class:on={group === g.kind} aria-pressed={group === g.kind} onclick={() => (group = g.kind)}>
								{g.label}
							</button>
						{/each}
					</div>
				</div>
				<div class="chips" role="group" aria-label="Workspaces">
					{#each workspaces as w (w.id)}
						<button type="button" class="chip" class:on={!hidden.has(w.id)} aria-pressed={!hidden.has(w.id)} onclick={() => toggleWorkspace(w.id)}>
							{w.label} <span class="n">{w.open}</span>
						</button>
					{/each}
				</div>
				<label class="done-toggle"><input type="checkbox" bind:checked={showDone} /> show done</label>
			</div>
			{#if contexts.length > 0}
				<div class="chips" role="group" aria-label="Filter by context">
					{#each contexts as [c, n] (c)}
						<button type="button" class="chip mono" class:on={context === c} aria-pressed={context === c} onclick={() => (context = context === c ? null : c)}>
							{c} <span class="n">{n}</span>
						</button>
					{/each}
				</div>
			{/if}

			{#if shown.length === 0}
				<div class="empty">
					<p><b>Nothing matches.</b></p>
					<p class="lede">Loosen the filters above.</p>
					<button type="button" class="chip on" onclick={resetFilters}>Reset filters</button>
				</div>
			{/if}

			<div class="groups">
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
							<UniversalRow row={r} {group} onOpen={open} onToggle={toggle} />
						{/each}
					</section>
				{/each}
			</div>
		{/if}
	</div>
</main>

<style>
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

	.caps {
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
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

	.controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 1rem;
		margin-top: 1.25rem;
	}

	.group-by {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.join {
		display: inline-flex;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		overflow: hidden;
	}

	.join button {
		padding: 0.15rem 0.6rem;
		border: 0;
		background: transparent;
		color: var(--color-base-content);
		font: inherit;
		font-size: 11px;
		letter-spacing: normal;
		text-transform: none;
		cursor: pointer;
	}

	.join button + button {
		border-left: 2px solid var(--color-base-content);
	}

	.join button.on,
	.chip.on {
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.375rem;
		margin-top: 0.75rem;
	}

	.controls .chips {
		margin-top: 0;
	}

	.chip {
		padding: 0.1rem 0.6rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: transparent;
		color: var(--color-base-content);
		font: inherit;
		font-size: 11px;
		font-weight: 700;
		cursor: pointer;
	}

	.n {
		opacity: 0.7;
	}

	.done-toggle {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin-left: auto;
		font-size: 0.75rem;
		cursor: pointer;
	}

	.done-toggle input {
		accent-color: var(--color-base-content);
	}

	.empty {
		margin-top: 2.5rem;
		padding: 2rem;
		border: 2px dashed color-mix(in srgb, var(--color-base-content) 30%, transparent);
		border-radius: var(--radius-box);
		text-align: center;
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
