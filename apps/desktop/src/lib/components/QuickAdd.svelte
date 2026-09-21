<script lang="ts">
	// Menu-bar quick-add window (tasks/desktop-quick-add, plan M7, plan §7 §3.2): mounts the same
	// `EditPopover` the main view uses, with no existing line and no anchor — "quick-add is a
	// second window around one shared component, not a second editor" (notes.md). The Rust side
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
	import { applyMutations, workspaceLayout } from "$lib/daemon";
	import EditPopover from "./EditPopover.svelte";

	// Quick-add always appends to the root file — there is no "current file" concept for a
	// global, no-document-behind-it popover (notes.md: "no document behind it"). Which file that is
	// comes from the workspace's layout (`todo_file`), refetched on every show since the workspace
	// can change while this window is hidden. This window has its own JS context, so the main
	// window's layout store is not shared.
	let rootPath = $state("todo.txt");

	async function refreshRootPath() {
		try {
			rootPath = (await workspaceLayout()).todo_file;
		} catch {
			// Keep the last known root list: a failed fetch must not stop a capture.
		}
	}

	// Bumped on every `quick-add-shown` event (emitted by the Rust shortcut handler right after
	// showing+focusing this window) so `{#key}` remounts `EditPopover` with a fresh, empty,
	// focused input each time — this window is hidden, not destroyed, between uses, so without
	// this its state would otherwise carry over from the previous open.
	let showCount = $state(0);
	let unlisten: UnlistenFn | undefined;

	async function hide() {
		await getCurrentWindow().hide();
	}

	/** The daemon stamps the creation date and `id:` — this only ever sends the raw text (design
	 * §7, notes.md: "the daemon stamps the creation date and id: — never the UI"). An empty/no-op
	 * submit never reaches here: `EditPopover`'s own `isNoOpEdit(initialLine, next)` check (against
	 * `initialLine = ""`) already turns that into a plain cancel. */
	async function handleSave(line: string) {
		await applyMutations(rootPath, [{ kind: "add", line }]);
		await hide();
	}

	async function handleCancel() {
		await hide();
	}

	onMount(async () => {
		void refreshRootPath();
		unlisten = await listen("quick-add-shown", () => {
			showCount++;
			void refreshRootPath();
		});
	});

	onDestroy(() => {
		unlisten?.();
	});
</script>

<div class="quick-add">
	{#key showCount}
		<EditPopover
			path={rootPath}
			initialLine=""
			taskRef={null}
			anchor={null}
			onSave={handleSave}
			onCancel={handleCancel}
		/>
	{/key}
</div>

<style>
	.quick-add {
		display: flex;
		width: 100%;
		height: 100%;
	}

	/* This window has no document behind it (notes.md) — the popover fills it edge to edge
	   instead of floating as an anchored overlay the way the main view's does. */
	.quick-add :global(.popover) {
		position: static;
		width: 100%;
		box-shadow: none;
	}
</style>
