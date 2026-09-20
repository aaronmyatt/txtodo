<script lang="ts">
	// Activity pane (ADR 0004 oplog.db, plan M7): renders op_log() — a one-shot bounded fetch
	// (newest ~200, newest first; see apps/desktop/src-tauri/src/commands_activity.rs), not a live
	// stream. So this pane fetches on mount, refetches on window focus, and offers a manual
	// refresh button — never a fabricated row while a fetch is pending or the log is empty.
	import { onMount } from "svelte";
	import { opLog } from "./api";
	import { relativeTime } from "./time";
	import type { OpEvent } from "./types";

	let entries = $state<OpEvent[]>([]);
	let loading = $state(true);
	let error = $state("");
	let lastRefreshedMs = $state(0);

	async function load() {
		loading = true;
		error = "";
		try {
			entries = await opLog();
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

<section class="activity">
	<div class="header">
		<h2>Activity</h2>
		<button type="button" onclick={load} disabled={loading}>{loading ? "Refreshing…" : "Refresh"}</button>
	</div>

	{#if lastRefreshedMs}
		<p class="hint">Last refreshed {relativeTime(lastRefreshedMs)}</p>
	{/if}

	{#if loading && entries.length === 0}
		<p>Loading…</p>
	{:else if error}
		<p class="state-error">{error}</p>
	{:else if entries.length === 0}
		<p>No recent activity.</p>
	{:else}
		<ul>
			{#each entries as entry, i (`${entry.principal}-${entry.at_ms}-${i}`)}
				<li>
					<span class="principal">{entry.principal}</span>
					<span class="op">{entry.op}</span>
					{#if entry.source}<span class="source">{entry.source}</span>{/if}
					<span class="time">{relativeTime(entry.at_ms)}</span>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<style>
	.activity {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}
	.hint {
		color: var(--color-text-muted);
		font-size: 0.85rem;
	}
	ul {
		list-style: none;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}
	li {
		display: flex;
		gap: 0.75rem;
		border-bottom: 1px solid var(--color-border-subtle);
		padding: 0.35rem 0;
	}
	.principal {
		font-family: monospace;
		color: var(--color-text-secondary);
	}
	.op {
		flex: 1;
	}
	.source {
		color: var(--color-text-muted);
		white-space: nowrap;
	}
	.time {
		color: var(--color-text-muted);
		white-space: nowrap;
	}
	.state-error {
		color: var(--color-danger);
		font-weight: 600;
	}
</style>
