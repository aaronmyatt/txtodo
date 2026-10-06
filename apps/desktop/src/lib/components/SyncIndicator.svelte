<script lang="ts">
	// The status footer's sync indicator (specs/client-parity.toml sync.open; the TUI's `s`): a dot
	// and the summary line, live (polled every few seconds; no change listener, which at startup
	// cost the 10k-line first paint ~500 ms), and a popup with
	// one block per peer. `s` in list mode or a click opens it; Esc or a click outside closes it.
	import { onMount } from "svelte";
	import { syncStatus, type SyncStatus } from "$lib/daemon";
	import { onListCommand } from "$lib/listCommands";
	import { peerLines, syncSummary } from "$lib/sync/summary";

	const POLL_MS = 5000;

	let status = $state<SyncStatus | null>(null);
	let open = $state(false);
	let root: HTMLElement | undefined = $state();
	const summary = $derived(status ? syncSummary(status) : null);

	async function refresh() {
		try {
			status = await syncStatus();
		} catch {
			status = null; // no daemon: the indicator hides rather than guess
		}
	}

	function onWindowKeydown(e: KeyboardEvent) {
		if (open && e.key === "Escape") open = false;
	}

	function onWindowPointerdown(e: PointerEvent) {
		if (open && root && !root.contains(e.target as Node)) open = false;
	}

	// The first read waits a moment: at startup a SyncStatus call queues behind the buffer's own
	// GetFile and cost the 10k-line first paint ~500 ms (e2e/perf.spec.ts).
	const FIRST_READ_MS = 1500;

	onMount(() => {
		// setTimeout / setInterval: https://developer.mozilla.org/en-US/docs/Web/API/Window/setInterval
		const first = setTimeout(() => void refresh(), FIRST_READ_MS);
		const timer = setInterval(() => void refresh(), POLL_MS);
		const offKey = onListCommand((id) => {
			if (id === "sync.open") open = !open;
		});
		return () => {
			clearTimeout(first);
			clearInterval(timer);
			offKey();
		};
	});
</script>

<svelte:window onkeydown={onWindowKeydown} onpointerdown={onWindowPointerdown} />

{#if summary && status}
	<span class="sync" bind:this={root}>
		<button type="button" class="indicator" aria-expanded={open} aria-label="Sync: {summary.text}" onclick={() => (open = !open)}>
			<span class="dot {summary.tone}" aria-hidden="true"></span>{summary.text}
		</button>
		{#if open}
			<div class="popup" role="dialog" aria-label="Sync details">
				<p class="title">Sync</p>
				<p>{summary.text}</p>
				{#each status.peers as p (p.device)}
					<ul>
						{#each peerLines(p, status.protocol) as line, i (i)}
							<li class:first={i === 0}>{line}</li>
						{/each}
					</ul>
				{/each}
			</div>
		{/if}
	</span>
{/if}

<style>
	.sync {
		position: relative;
	}

	.indicator {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.1rem 0.4rem;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font: inherit;
		cursor: pointer;
	}

	.indicator:hover {
		background: color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	.dot {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 999px;
		background: var(--color-success, #15803d);
	}

	.dot.busy {
		background: var(--color-warning, #d97706);
	}

	.dot.bad {
		background: var(--color-error, #dc2626);
	}

	.popup {
		position: absolute;
		bottom: calc(100% + 0.5rem);
		left: 0;
		z-index: 40;
		min-width: 18rem;
		padding: 0.75rem 1rem;
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
		font-size: 0.75rem;
	}

	.popup p {
		margin: 0 0 0.5rem;
	}

	.title {
		font-weight: 700;
	}

	ul {
		margin: 0.5rem 0 0;
		padding: 0.5rem 0 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
		list-style: none;
	}

	li {
		padding-left: 1rem;
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
	}

	li.first {
		padding-left: 0;
		color: var(--color-base-content);
		font-weight: 700;
	}
</style>
