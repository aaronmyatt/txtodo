<script lang="ts">
	import type { Snippet } from "svelte";
	// Tailwind/DaisyUI layers first, so app.css (unlayered) still wins wherever both style a thing.
	import "../tailwind.css";
	import "../app.css";
	// JetBrains Mono, the brand typeface (task desktop-ui-revamp): 400/500 for the buffer and the
	// chrome (--font-mono), 700 for the wordmark and headings (--font-brand). Bundled by Vite from
	// the Fontsource package, latin only. Self-hosted, never Google Fonts: the app and the visual
	// goldens never touch the network for it (a slow fetch timed out the screenshots' font wait, and
	// offline the wordmark fell back to another font). OFL-1.1.
	// Ref: https://fontsource.org/fonts/jetbrains-mono/install
	import "@fontsource/jetbrains-mono/latin-400.css";
	import "@fontsource/jetbrains-mono/latin-500.css";
	import "@fontsource/jetbrains-mono/latin-700.css";
	import { resolvedTheme } from "$lib/stores/theme";
	import { editorPrefs } from "$lib/stores/editorPrefs";
	import { isQuickAddWindow } from "$lib/windowLabel";
	import AppShell from "$lib/components/AppShell.svelte";

	// The quick-add window (routes/+page.svelte) is a bare popover: no header. Outside a Tauri
	// webview (`npm run dev`, Playwright) there is no window label, so the shell shows.
	const isQuickAdd = isQuickAddWindow();

	let { children }: { children: Snippet } = $props();

	// `<html data-theme>` is what app.css's `[data-theme="dark"]` overrides key off — set here
	// (rather than in app.html before hydration) since the resolved value depends on
	// localStorage/matchMedia, neither available at prerender time under adapter-static.
	$effect(() => {
		document.documentElement.dataset.theme = $resolvedTheme;
	});

	// The editor's sizes, for every FileView and the detail parent line (`$lib/stores/editorPrefs`).
	$effect(() => {
		const root = document.documentElement.style;
		root.setProperty("--editor-font-size", `${$editorPrefs.fontSize}px`);
		root.setProperty("--editor-line-height", `${$editorPrefs.lineHeight}px`);
	});
</script>

{#if isQuickAdd}
	{@render children()}
{:else}
	<AppShell>{@render children()}</AppShell>
{/if}
