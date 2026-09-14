<script lang="ts">
	import type { Snippet } from "svelte";
	import "../app.css";
	import { resolvedTheme } from "$lib/stores/theme";

	let { children }: { children: Snippet } = $props();

	// `<html data-theme>` is what app.css's `[data-theme="dark"]` overrides key off — set here
	// (rather than in app.html before hydration) since the resolved value depends on
	// localStorage/matchMedia, neither available at prerender time under adapter-static.
	$effect(() => {
		document.documentElement.dataset.theme = $resolvedTheme;
	});
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
