<script lang="ts">
	// Duplicate-lines banner (ADR 0032): the open document's lines that read the same, counted,
	// with Review opening the sheet. The `needs_review` flags this banner also showed went with the
	// Loro mirror (ADR 0038, conflict review A): nothing raises them any more.
	//
	// Groups are read with `listDuplicates` on mount and after a change to this file that has or
	// had any (a `Change` only counts them). A group never locks the buffer.
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

	/** Groups are derived by the daemon when asked; an older daemon (or the e2e bridge before it
	 * learned the command) has none. */
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

	async function closeSheet() {
		sheetOpen = false;
		await syncGroups(); // true up in case a keep landed while the sheet was open
	}
</script>

{#if groups.length > 0}
	<div class="banner tx-strip" role="status">
		<span>{groupsLabel(groups.length)}</span>
		<button type="button" onclick={() => (sheetOpen = true)}>Review</button>
	</div>
{/if}

{#if sheetOpen && groups.length > 0}
	<ConflictReviewSheet {path} {groups} onClose={closeSheet} onGroupResolved={syncGroups} />
{/if}
