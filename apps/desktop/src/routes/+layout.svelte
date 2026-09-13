<script lang="ts">
	import type { Snippet } from "svelte";

	let { children }: { children: Snippet } = $props();
</script>

<svelte:head>
	<style>
		/* Zeroes the browser's default ~8px body margin so a full-bleed view (MainView's editor)
		   truly reaches the window edge — app.html's wrapper div is `display: contents`, so this is
		   the only place a stray margin could sneak in. */
		:global(body) {
			margin: 0;
		}
	</style>
</svelte:head>

{@render children()}
