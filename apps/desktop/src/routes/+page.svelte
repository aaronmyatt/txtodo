<script lang="ts">
	// The root route mounts one of two whole-window components, chosen by which Tauri window
	// loaded it (tasks/desktop-quick-add/notes.md): the real main view (tasks/desktop-main-view;
	// see $lib/components/MainView.svelte for the daemon banner, file view and edit popover) for
	// every ordinary window, or the menu-bar quick-add popover for the hidden "quick-add" window
	// the Rust side creates at startup and shows on the global hotkey (src-tauri/src/lib.rs).
	//
	// One static build serving both windows by label, rather than a second SvelteKit route, is a
	// deliberate choice — see QuickAdd.svelte's module doc for why.
	import { isQuickAddWindow } from "$lib/windowLabel";
	import MainView from "$lib/components/MainView.svelte";
	import QuickAdd from "$lib/components/QuickAdd.svelte";

	const isQuickAdd = isQuickAddWindow();
</script>

{#if isQuickAdd}
	<QuickAdd />
{:else}
	<MainView />
{/if}
