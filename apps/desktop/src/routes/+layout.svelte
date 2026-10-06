<script lang="ts">
	import { onMount, type Snippet } from "svelte";
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
	import { describeError, installCrashLog } from "$lib/crashLog";
	import { logError } from "$lib/tauriShim";

	// The quick-add window (routes/+page.svelte) is a bare popover: no header. Outside a Tauri
	// webview (`npm run dev`, Playwright) there is no window label, so the shell shows.
	const isQuickAdd = isQuickAddWindow();

	let { children }: { children: Snippet } = $props();

	// Uncaught errors and unhandled rejections land in desktop.log (`$lib/crashLog.ts`).
	onMount(() => installCrashLog(window, logError));

	/** A render error inside the page: log it, and show a way back instead of a blank window.
	 * https://svelte.dev/docs/svelte/svelte-boundary */
	function onPageError(error: unknown) {
		logError("render error", { ...describeError(error) });
	}

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

{#snippet page()}
	<svelte:boundary onerror={onPageError}>
		{@render children()}
		{#snippet failed(error, reset)}
			<div class="crashed" role="alert">
				<p><b>This page broke.</b> The error is in desktop.log.</p>
				<p class="why">{describeError(error).message}</p>
				<button type="button" onclick={reset}>Try again</button>
			</div>
		{/snippet}
	</svelte:boundary>
{/snippet}

{#if isQuickAdd}
	{@render page()}
{:else}
	<AppShell>{@render page()}</AppShell>
{/if}

<style>
	.crashed {
		margin: 3rem auto;
		max-width: 32rem;
		padding: 1.5rem;
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		font-family: var(--font-mono);
	}

	.crashed p {
		margin: 0 0 0.75rem;
	}

	.why {
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-size: 0.8125rem;
	}

	.crashed button {
		padding: 0.25rem 0.9rem;
		border: 0;
		border-radius: 999px;
		background: var(--color-primary);
		color: var(--color-primary-content);
		font: inherit;
		font-weight: 700;
		cursor: pointer;
	}
</style>
