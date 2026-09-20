<script lang="ts">
	// This app's version and release date, small and muted, plus a banner when the running daemon
	// is another build (task version-info). `banner` picks which half this instance shows, so the
	// main view can put the banner with its other banners and the label in a corner.
	import { onMount } from "svelte";
	import { buildInfo } from "$lib/daemon";
	import { daemonMismatch, versionLabel, type BuildInfo } from "$lib/versionInfo";

	let { banner = false }: { banner?: boolean } = $props();
	let info = $state<BuildInfo | null>(null);
	const warning = $derived(info ? daemonMismatch(info) : null);

	onMount(() => {
		// Best effort: with no bridge (or an older one) the label simply does not show.
		buildInfo()
			.then((i) => (info = i))
			.catch(() => {});
	});
</script>

{#if banner}
	{#if warning}
		<p class="daemon-mismatch" role="status">{warning}</p>
	{/if}
{:else if info}
	<span class="version" title="This app's version and release date">{versionLabel(info.version, info.release_date)}</span>
{/if}

<style>
	.version {
		color: var(--color-text-muted, #6b7280);
		font-size: 0.7rem;
		opacity: 0.8;
		white-space: nowrap;
	}

	.daemon-mismatch {
		margin: 0.5rem 1rem;
		padding: 0.4rem 0.6rem;
		border: 1px solid var(--color-warning, #b45309);
		border-radius: 4px;
		color: var(--color-text, inherit);
		font-size: 0.85rem;
	}
</style>
