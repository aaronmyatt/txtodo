<script lang="ts">
	// A paired device on another sync protocol (task sync-divergence-check/protocol-mismatch): the
	// two refuse each other and nothing syncs until the older one is upgraded, so this says which
	// one. `SyncStatus` comes on no `Watch` event, so it is polled. Not dismissable: it never heals
	// by itself. A failed read (no daemon, an older daemon) shows nothing; the daemon banner
	// already covers a missing daemon.
	// Ref (setInterval): https://developer.mozilla.org/en-US/docs/Web/API/Window/setInterval
	import { onDestroy, onMount } from "svelte";
	import { syncStatus } from "$lib/daemon";
	import { protocolBanner, type ProtocolBanner } from "$lib/syncProtocol";

	/** How often to re-read: the daemon re-dials a peer about this often (LAN resync, relay). */
	const POLL_MS = 10_000;

	let banner = $state<ProtocolBanner | null>(null);
	let timer: ReturnType<typeof setInterval> | undefined;

	async function refresh() {
		try {
			banner = protocolBanner(await syncStatus());
		} catch {
			banner = null;
		}
	}

	onMount(() => {
		void refresh();
		timer = setInterval(() => void refresh(), POLL_MS);
	});

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});
</script>

{#if banner}
	<div class="banner" role="alert">
		<strong>{banner.label}</strong>
		<span>{banner.detail}</span>
	</div>
{/if}

<style>
	.banner {
		display: flex;
		align-items: baseline;
		gap: 0.75rem;
		background: var(--color-attention-bg);
		color: var(--color-attention-text);
		padding: 0.5rem 1rem;
		border-radius: 6px;
		margin: 0.75rem 1rem;
	}

	.banner span {
		flex: 1;
	}
</style>
