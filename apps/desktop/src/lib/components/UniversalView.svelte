<script lang="ts">
	// Universal view (ADR 0025, task desktop-universal-view): every registered workspace's open
	// tasks in one flat list from `universalTasks()`, grouped by priority and filterable by
	// `@context` — both done here, over the daemon's unsorted result (see $lib/daemon.ts's own doc
	// comment on why grouping is the caller's job). Clicking a task switches to its workspace and
	// hands MainView a `PendingUniversalNav` so it opens straight to that line, tagged with
	// `workspaceRoot` so `Breadcrumb` shows the owning project — see MainView.svelte's
	// `$currentWorkspaceRoot` effect, the actual consumer.
	import { goto } from "$app/navigation";
	import { onMount } from "svelte";
	import { switchWorkspace, universalTasks, type UniversalTask } from "$lib/daemon";
	import { pendingUniversalNav } from "$lib/stores/workspaces";

	let tasks = $state<UniversalTask[]>([]);
	let loading = $state(true);
	let error = $state("");
	let activeContexts = $state<Set<string>>(new Set());

	const allContexts = $derived([...new Set(tasks.flatMap((t) => t.contexts))].sort());
	const filtered = $derived(
		activeContexts.size === 0 ? tasks : tasks.filter((t) => t.contexts.some((c) => activeContexts.has(c)))
	);
	const groups = $derived(groupByPriority(filtered));

	function groupByPriority(list: UniversalTask[]): [string, UniversalTask[]][] {
		const buckets = new Map<string, UniversalTask[]>();
		for (const task of list) {
			const key = task.priority ?? "";
			const bucket = buckets.get(key);
			if (bucket) bucket.push(task);
			else buckets.set(key, [task]);
		}
		// "" (no priority) sorts last, like todo.sh's own listing convention.
		return [...buckets.entries()].sort(([a], [b]) => (a || "~").localeCompare(b || "~"));
	}

	function toggleContext(ctx: string) {
		const next = new Set(activeContexts);
		if (next.has(ctx)) next.delete(ctx);
		else next.add(ctx);
		activeContexts = next;
	}

	/** Last path segment of an absolute root, for a compact label; falls back to the full path. */
	function projectLabel(root: string): string {
		return (
			root
				.split(/[/\\]/)
				.filter(Boolean)
				.pop() ?? root
		);
	}

	async function open(task: UniversalTask) {
		error = "";
		try {
			await switchWorkspace(task.workspace_root);
			pendingUniversalNav.set({
				file: "todo.txt",
				line: task.line_number,
				workspaceRoot: task.workspace_root
			});
			await goto("/");
		} catch (e) {
			error = String(e);
		}
	}

	onMount(async () => {
		try {
			tasks = await universalTasks();
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	});
</script>

<main class="universal">
	<div class="top-nav">
		<a href="/">‹ Back</a>
		<h1>Universal view</h1>
	</div>

	{#if error}
		<p class="error" role="alert">{error}</p>
	{/if}

	{#if loading}
		<p>Loading…</p>
	{:else}
		{#if allContexts.length > 0}
			<div class="contexts" role="group" aria-label="Filter by context">
				{#each allContexts as ctx (ctx)}
					<button
						type="button"
						class="chip"
						class:active={activeContexts.has(ctx)}
						onclick={() => toggleContext(ctx)}
					>
						{ctx}
					</button>
				{/each}
			</div>
		{/if}

		{#if filtered.length === 0}
			<p>
				No open tasks{activeContexts.size > 0
					? " for the selected contexts"
					: " across any registered workspace"}.
			</p>
		{/if}

		{#each groups as [priority, group] (priority)}
			<section>
				<h2>{priority ? `(${priority})` : "No priority"}</h2>
				<ul>
					{#each group as task (task.workspace_id + ':' + task.line_number + ':' + task.description)}
						<li>
							<button type="button" class="task" onclick={() => open(task)}>
								<span class="description">{task.description}</span>
								<span class="meta">
									<span class="project">{projectLabel(task.workspace_root)}</span>
									{#each task.contexts as ctx (ctx)}
										<span class="context">{ctx}</span>
									{/each}
								</span>
							</button>
						</li>
					{/each}
				</ul>
			</section>
		{/each}
	{/if}
</main>

<style>
	.universal {
		display: flex;
		flex-direction: column;
		gap: 1.25rem;
		padding: 1.5rem 2rem 3rem;
		height: 100vh;
		overflow-y: auto;
		background: var(--color-bg);
		color: var(--color-text);
		box-sizing: border-box;
	}

	.top-nav {
		display: flex;
		align-items: baseline;
		gap: 1rem;
	}

	.top-nav h1 {
		margin: 0;
	}

	.contexts {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}

	.chip {
		background: transparent;
		border: 1px solid var(--color-border, currentColor);
		border-radius: 999px;
		padding: 0.15rem 0.65rem;
		color: inherit;
		cursor: pointer;
		font-size: 0.85rem;
	}

	.chip.active {
		background: var(--color-hover-bg, rgba(128, 128, 128, 0.25));
		font-weight: 600;
	}

	section h2 {
		font-size: 1rem;
		opacity: 0.75;
		margin: 0 0 0.4rem;
	}

	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.task {
		width: 100%;
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 1rem;
		text-align: left;
		background: transparent;
		border: none;
		border-radius: 6px;
		padding: 0.45rem 0.6rem;
		color: inherit;
		cursor: pointer;
	}

	.task:hover {
		background: var(--color-hover-bg, rgba(128, 128, 128, 0.15));
	}

	.description {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.meta {
		display: flex;
		gap: 0.4rem;
		flex-shrink: 0;
		opacity: 0.7;
		font-size: 0.85rem;
	}

	.project {
		font-style: italic;
	}

	.error {
		color: var(--color-banner-text, crimson);
	}
</style>
