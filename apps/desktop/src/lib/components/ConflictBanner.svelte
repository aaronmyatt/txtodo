<script lang="ts">
	// Conflict banner (plan M7, design §4.7): counts one open document's duplicate groups (ADR
	// 0032: lines that read the same) and opens the review sheet on click. Same-word review went
	// with the todo.txt mirror (ADR 0038), so duplicates are all it shows.
	//
	// Groups are read with `listDuplicates` on mount and after a change to this file that has or
	// had any (a `Change` only counts them). `watch()` starts the daemon's `Watch` stream and
	// `onDaemonChange` listens for the `daemon-change` event it forwards.
	// Ref: https://tauri.app/reference/javascript/event/#listen
	import { onDestroy, onMount } from "svelte";
	import type { UnlistenFn } from "@tauri-apps/api/event";
	import { listDuplicates, onDaemonChange, watch, type DuplicateGroup } from "$lib/daemon";
	import { groupsLabel } from "$lib/stores/duplicates";
	import ConflictReviewSheet from "./ConflictReviewSheet.svelte";

	let { path }: { path: string } = $props();

	let groups = $state<DuplicateGroup[]>([]);
	let sheetOpen = $state(false);
	let unlisten: UnlistenFn | undefined;

	/** Groups are derived by the daemon when asked; an older daemon has none, which must not break
	 * the view. */
	async function syncGroups() {
		try {
			groups = (await listDuplicates(path)).filter((g) => g.tasks.length > 1);
		} catch {
			groups = [];
		}
	}

	onMount(async () => {
		await syncGroups();
		await watch();
		unlisten = await onDaemonChange((change) => {
			if (change.path === path && ((change.duplicate_groups ?? 0) > 0 || groups.length > 0)) {
				void syncGroups();
			}
		});
	});

	onDestroy(() => {
		unlisten?.();
	});

	function openSheet() {
		sheetOpen = true;
	}

	async function closeSheet() {
		sheetOpen = false;
		await syncGroups(); // true up in case a keep landed while the sheet was open
	}
</script>

{#if groups.length > 0}
	<div class="banner tx-strip" role="status">
		<span>{groupsLabel(groups.length)}</span>
		<button type="button" onclick={openSheet}>Review</button>
	</div>
{/if}

{#if sheetOpen && groups.length > 0}
	<ConflictReviewSheet {path} {groups} onClose={closeSheet} onGroupResolved={syncGroups} />
{/if}
