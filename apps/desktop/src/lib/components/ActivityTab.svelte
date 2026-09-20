<script lang="ts">
	// Cross-workspace activity feed (task desktop-activity-cross-workspace, storyboard screen 08):
	// the nav sidebar's Activity tab, scaffolded as a placeholder by desktop-workspace-nav-sidebar.
	// Same bounded-fetch contract as devices/ActivityFeed.svelte's single-workspace feed (fetch on
	// mount, refetch on window focus, manual refresh button) — deliberately not a live tail, since
	// nothing on the daemon side pushes op-log events yet (that component's own module doc).
	import { onMount } from "svelte";
	import { opLogAll, type AggregatedOpLogEntry } from "$lib/daemon";
	import { relativeTime } from "../../devices/time";
	import { isAgent, shortRoot } from "./activityLogic";

	let entries = $state<AggregatedOpLogEntry[]>([]);
	let loading = $state(true);
	let error = $state("");
	let lastRefreshedMs = $state(0);

	async function load() {
		loading = true;
		error = "";
		try {
			entries = await opLogAll();
			lastRefreshedMs = Date.now();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function onFocus() {
		load();
	}

	onMount(() => {
		load();
		window.addEventListener("focus", onFocus);
		return () => window.removeEventListener("focus", onFocus);
	});
</script>

<div class="activity">
	<div class="header">
		<button type="button" onclick={load} disabled={loading}>
			{loading ? "Refreshing…" : "Refresh"}
		</button>
		{#if lastRefreshedMs}
			<p class="hint">Last refreshed {relativeTime(lastRefreshedMs)}</p>
		{/if}
	</div>

	{#if loading && entries.length === 0}
		<p>Loading…</p>
	{:else if error}
		<p class="state-error" role="alert">{error}</p>
	{:else if entries.length === 0}
		<p>No recent activity.</p>
	{:else}
		<ul>
			{#each entries as entry, i (`${entry.workspace_id}-${entry.principal}-${entry.at_ms}-${i}`)}
				<li>
					<span class="workspace-chip" title={entry.workspace_root}>{shortRoot(entry.workspace_root)}</span>
					<span class="principal" class:agent={isAgent(entry.principal)}>{entry.principal}</span>
					<span class="op">{entry.op}</span>
					{#if entry.source}<span class="source">{entry.source}</span>{/if}
					<span class="time">{relativeTime(entry.at_ms)}</span>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.activity {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		flex: 1;
		overflow-y: auto;
	}

	.header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.hint {
		margin: 0;
		color: var(--color-text-muted);
		font-size: 0.85rem;
	}

	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	li {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0.5rem;
		border-bottom: 1px solid var(--color-border-subtle);
		padding: 0.35rem 0;
	}

	.workspace-chip {
		background: var(--color-surface-muted);
		border-radius: 4px;
		padding: 0.1rem 0.4rem;
		font-size: 0.75rem;
		font-family: monospace;
	}

	.principal {
		font-family: monospace;
		color: var(--color-text-secondary);
	}

	/* Agent-vs-human distinction (notes.md): a small pill style, no new backend signal. */
	.principal.agent {
		background: var(--color-attention-bg);
		color: var(--color-attention-text);
		border-radius: 4px;
		padding: 0.05rem 0.35rem;
	}

	.op {
		flex: 1;
		min-width: 8rem;
	}

	.source {
		color: var(--color-text-muted);
		white-space: nowrap;
		font-size: 0.85rem;
	}
	.time {
		color: var(--color-text-muted);
		white-space: nowrap;
		font-size: 0.85rem;
	}

	.state-error {
		color: var(--color-danger);
		font-weight: 600;
	}
</style>
