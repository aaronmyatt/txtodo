<script lang="ts">
	// Conflict banner (plan M7, design §4.7): shows the count of pending needs_review flags for
	// one open document and opens the review sheet on click.
	//
	// Subscribes to the daemon's `Watch` stream the same way the main view does (per this task's
	// brief) — `watch()` starts the stream, `onDaemonChange` listens for the `daemon-change` event
	// it forwards. Ref: https://tauri.app/reference/javascript/event/#listen
	//
	// Dismissing this banner only hides it for the current session: `dismissed` is local component
	// state, never written to `pendingConflicts`. The flag stays in the store (and in
	// `list_conflicts`) until a real resolve op clears it — design §4.7's invariant is "dismissing
	// the banner or navigating away never clears a flag".
	//
	// It also counts the file's duplicate groups (ADR 0032: lines that read the same), read with
	// `listDuplicates` on mount and after a change to this file that has or had any (a `Change`
	// only counts them). A group opens the same sheet but never locks the buffer.
	import { onDestroy, onMount } from "svelte";
	import type { UnlistenFn } from "@tauri-apps/api/event";
	import {
		listConflicts,
		listDuplicates,
		onDaemonChange,
		watch,
		type DuplicateGroup
	} from "$lib/daemon";
	import { flagsForPath, pendingConflicts } from "$lib/stores/conflicts";
	import { groupsLabel } from "$lib/stores/duplicates";
	import ConflictReviewSheet from "./ConflictReviewSheet.svelte";

	let { path }: { path: string } = $props();

	let dismissed = $state(false);
	let groups = $state<DuplicateGroup[]>([]);
	let sheetOpen = $state(false);
	let unlisten: UnlistenFn | undefined;

	const flags = $derived(flagsForPath($pendingConflicts, path));

	// A fresh conflict is a fresh thing to review even if an earlier batch was dismissed.
	$effect(() => {
		if (flags.length > 0) dismissed = false;
	});

	/** Groups are derived by the daemon when asked; an older daemon (or the e2e bridge before it
	 * learned the command) has none, which must not break the flags banner. */
	async function syncGroups() {
		try {
			groups = (await listDuplicates(path)).filter((g) => g.tasks.length > 1);
		} catch {
			groups = [];
		}
	}

	/** The daemon is authoritative (design §4.7): true up the store against `list_conflicts`
	 * rather than trusting only what `Watch` events have accumulated so far. */
	async function syncFromDaemon() {
		const fresh = await listConflicts(path);
		pendingConflicts.replaceForPath(path, fresh);
		await syncGroups();
	}

	onMount(async () => {
		await syncFromDaemon();
		await watch();
		unlisten = await onDaemonChange((change) => {
			if (change.path === path && change.review.length > 0) {
				pendingConflicts.addFlags(path, change.review);
			}
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
		await syncFromDaemon(); // true up in case a resolve landed while the sheet was open
	}

	function dismiss() {
		dismissed = true; // visibility only — see module note above; never touches the store
	}
</script>

{#if flags.length > 0 && !dismissed}
	<div class="banner" role="status">
		<span>{flags.length} task{flags.length === 1 ? "" : "s"} need review</span>
		<button type="button" onclick={openSheet}>Review</button>
		<button type="button" class="dismiss" onclick={dismiss} aria-label="Dismiss">×</button>
	</div>
{/if}

{#if groups.length > 0}
	<div class="banner" role="status">
		<span>{groupsLabel(groups.length)}</span>
		<button type="button" onclick={openSheet}>Review</button>
	</div>
{/if}

{#if sheetOpen && (flags.length > 0 || groups.length > 0)}
	<ConflictReviewSheet {path} {flags} {groups} onClose={closeSheet} onGroupResolved={syncGroups} />
{/if}

<style>
	.banner {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		background: var(--color-attention-bg);
		color: var(--color-attention-text);
		padding: 0.5rem 1rem;
		border-radius: 6px;
		margin: 0.75rem 1rem;
	}

	.banner span {
		flex: 1;
	}

	.dismiss {
		background: transparent;
		border: none;
		font-size: 1.1rem;
		cursor: pointer;
		color: inherit;
	}
</style>
