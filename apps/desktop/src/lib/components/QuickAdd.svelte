<script lang="ts">
	// Menu-bar quick-add window (tasks/desktop-quick-add, plan M7, plan §7 §3.2): mounts the main
	// window's `PromptBar` in its windowed form (task desktop-ui-revamp) — "quick-add is a second
	// window around one shared component, not a second editor" (notes.md). The Rust side
	// (`src-tauri/src/lib.rs`) creates this window hidden at startup and shows/focuses it on the
	// global hotkey; this component only cares about what happens once it's visible.
	//
	// This file is the window's *whole* content: `src/routes/+page.svelte` renders it instead of
	// `MainView` when `getCurrentWindow().label === "quick-add"` (see that file) — one static
	// `index.html` build serves both windows, like Tauri's own default single-window setup, rather
	// than adding a second SvelteKit route whose prerendering behaviour under `adapter-static`
	// would need re-verifying every time the build config changes.
	import { listen, type UnlistenFn } from "@tauri-apps/api/event";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { onDestroy, onMount } from "svelte";
	import { workspaceLayout } from "$lib/daemon";
	import PromptBar from "./PromptBar.svelte";

	// Quick-add always appends to the root file — there is no "current file" concept for a
	// global, no-document-behind-it popover (notes.md: "no document behind it"). Which file that is
	// comes from the workspace's layout (`todo_file`), asked for again right before every save
	// (task layout-hot-reload-clients): a capture typed before the first answer, or after the
	// layout changed, used to land in a file the main view never shows. This window has its own
	// JS context, so the main window's layout store is not shared.

	/** The root list right now; throws when the daemon cannot say, so a capture is never written
	 * to a guessed file. */
	async function currentRootPath(): Promise<string> {
		return (await workspaceLayout()).todo_file || "todo.txt";
	}

	// Bumped on every `quick-add-shown` event (emitted by the Rust shortcut handler right after
	// showing+focusing this window) so `{#key}` remounts `PromptBar` with a fresh, empty,
	// focused input each time — this window is hidden, not destroyed, between uses, so without
	// this its state would otherwise carry over from the previous open.
	let showCount = $state(0);
	let unlisten: UnlistenFn | undefined;

	async function hide() {
		await getCurrentWindow().hide();
	}

	// The daemon stamps the creation date and `id:` — the bar only ever sends the raw text (design
	// §7). An empty submit never reaches the daemon: the bar drops blank lines itself.
	function hideWindow() {
		void hide();
	}

	onMount(async () => {
		unlisten = await listen("quick-add-shown", () => {
			showCount++;
		});
	});

	onDestroy(() => {
		unlisten?.();
	});
</script>

<div class="quick-add">
	{#key showCount}
		<PromptBar windowed resolvePath={currentRootPath} onAdded={hideWindow} onEscape={hideWindow} />
	{/key}
</div>

<style>
	.quick-add {
		height: 100vh;
		background: var(--color-base-100);
		color: var(--color-base-content);
	}
</style>
