<script lang="ts">
	// Cross-workspace activity feed (task desktop-activity-cross-workspace, storyboard screen 08):
	// Settings > Activity since task desktop-ui-revamp (it was the workspace menu's Activity tab).
	// The newest ops (up to 200 per call) across every registered workspace, each with a glyph for
	// who made it.
	// A bounded fetch (on mount, on window focus, and the Refresh button), deliberately not a live
	// tail: nothing on the daemon side pushes op-log events yet.
	import { onMount } from "svelte";
	import { opLogAll, type AggregatedOpLogEntry } from "$lib/daemon";
	import Icon from "$lib/icons/Icon.svelte";
	import { relativeTime } from "../../../devices/time";
	import { principalKind, shortRoot } from "../activityLogic";

	const GLYPH = { you: "laptop", agent: "bolt", external: "file" } as const;

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
		<p class="hint">
			Every workspace, newest first.{#if lastRefreshedMs}{" "}Refreshed {relativeTime(lastRefreshedMs)}.{/if}
		</p>
		<button type="button" class="pill" onclick={load} disabled={loading}>
			{loading ? "Refreshing…" : "Refresh"}
		</button>
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
					<span class="glyph {principalKind(entry.principal)}" title={entry.principal}>
						<Icon name={GLYPH[principalKind(entry.principal)]} />
					</span>
					<span class="workspace-chip" title={entry.workspace_root}>{shortRoot(entry.workspace_root)}</span>
					<span class="principal">{entry.principal}</span>
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
		gap: 0.75rem;
	}

	.header {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
	}

	.hint,
	.source,
	.time {
		margin: 0;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-size: 0.75rem;
		white-space: nowrap;
	}

	.hint {
		font-family: var(--font-sans);
		white-space: normal;
	}

	ul {
		display: flex;
		flex-direction: column;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 0.6rem;
		padding: 0.45rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
		font-size: 0.8125rem;
	}

	li:first-child {
		border-top: 0;
	}

	/* Who did it: you (ink), an agent (inverse), an edit from outside txtodo (dashed). */
	.glyph {
		display: grid;
		flex-shrink: 0;
		place-items: center;
		width: 1.5rem;
		height: 1.5rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		--icon-size: 0.8rem;
	}

	.glyph.agent {
		background: var(--color-base-content);
		color: var(--color-base-100);
	}

	.glyph.external {
		border-style: dashed;
	}

	.workspace-chip {
		padding: 0 0.45rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 25%, transparent);
		border-radius: 0.5rem;
		font-size: 11px;
		font-weight: 700;
	}

	.principal {
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
	}

	.op {
		flex: 1;
		min-width: 8rem;
	}

	.state-error {
		color: var(--color-error, #dc2626);
		font-weight: 700;
	}
</style>
