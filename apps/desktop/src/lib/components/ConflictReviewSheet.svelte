<script lang="ts">
	// Conflict review sheet (plan M7; design §4.7 conflict table; plan §3.3 keyboard/a11y floor:
	// "popover traps focus"). A native `role="dialog"` with a hand-rolled focus trap — no new
	// dependency for it since the trap is ~15 lines (this task takes no new npm dependencies).
	// Ref (dialog pattern incl. focus trap + Escape): https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/
	import { onDestroy, onMount, tick } from "svelte";
	import DiffView from "./DiffView.svelte";
	import { diffText } from "$lib/wasmCore";
	import { resolveConflict, type ResolveChoice } from "$lib/daemon";
	import {
		isResurrectCandidate,
		pendingConflicts,
		type PendingConflict
	} from "$lib/stores/conflicts";

	let { path, flags, onClose }: { path: string; flags: PendingConflict[]; onClose: () => void } =
		$props();

	// Always review the first pending flag. `flags` is a reactive prop derived from the store
	// (see ConflictBanner), so resolving one shrinks it and this naturally advances to the next
	// without any index bookkeeping.
	const current = $derived(flags[0] as PendingConflict | undefined);

	let resolving = $state(false);
	let error = $state("");
	// Client-side-only reconstruction for the human to preview before tapping "keep merged" — see
	// the long comment on resolve() below for why this is never what gets sent to the daemon.
	let mergedPreview = $state("");

	let dialogEl: HTMLElement | undefined;
	let previouslyFocused: HTMLElement | null = null;

	async function buildMergedPreview(mine: string, theirs: string) {
		const segments = await diffText(mine, theirs);
		mergedPreview = segments.map((s) => s.text).join("");
	}

	$effect(() => {
		if (current) buildMergedPreview(current.mine, current.theirs);
	});

	// Auto-close once every flag this sheet was opened for has been resolved.
	$effect(() => {
		if (flags.length === 0) onClose();
	});

	function focusableElements(): HTMLElement[] {
		if (!dialogEl) return [];
		const selector = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';
		return Array.from(dialogEl.querySelectorAll<HTMLElement>(selector)).filter(
			(el) => !el.hasAttribute("disabled")
		);
	}

	// Focus trap + Esc-to-close (plan §3.3, ARIA dialog pattern linked above): Tab/Shift+Tab cycle
	// within the dialog instead of escaping focus to the page behind it.
	function onKeydown(e: KeyboardEvent) {
		if (e.key === "Escape") {
			e.preventDefault();
			onClose();
			return;
		}
		if (e.key !== "Tab") return;
		const els = focusableElements();
		if (els.length === 0) return;
		const first = els[0];
		const last = els[els.length - 1];
		if (e.shiftKey && document.activeElement === first) {
			e.preventDefault();
			last.focus();
		} else if (!e.shiftKey && document.activeElement === last) {
			e.preventDefault();
			first.focus();
		}
	}

	onMount(async () => {
		previouslyFocused = document.activeElement as HTMLElement | null;
		await tick(); // https://svelte.dev/docs/svelte/lifecycle-hooks#tick — wait for the dialog to render
		focusableElements()[0]?.focus();
	});

	onDestroy(() => {
		previouslyFocused?.focus();
	});

	/**
	 * Resolves the current flag through the daemon's `resolve` Tauri command — the same
	 * `Apply(Edit)` path the edit popover uses (design §4.7), so attribution/sync/history stay
	 * automatic and the op itself clears the flag server-side.
	 *
	 * `"merged"` is the one non-obvious case. `mergedPreview` above is a client-only interleave of
	 * `mine`/`theirs`, built purely to help the human decide, and it is NEVER sent anywhere —
	 * tapping "keep merged" still just calls `resolveConflict(path, task, "merged")` with no text
	 * payload. The daemon's own `ResolutionDto::Merged` doc comment
	 * (apps/desktop/src-tauri/src/dto.rs) spells out why: "keeps what is already in the file; only
	 * clears the flag". The char-level CRDT merge already landed in the file before the flag was
	 * ever raised (design §4.7: "edit word 3 | edit word 3 → char-level merge, flagged for a
	 * one-tap review"), so resolving as merged is an acknowledgement of what's already on disk,
	 * not a write of new content — this preview and that daemon call are deliberately two
	 * different things.
	 */
	async function resolve(choice: ResolveChoice) {
		if (!current || resolving) return;
		resolving = true;
		error = "";
		try {
			await resolveConflict(
				path,
				{ line_number: current.line_number, task_id: current.task_id },
				choice
			);
			pendingConflicts.remove(current.task_id);
		} catch (e) {
			error = String(e);
		} finally {
			resolving = false;
		}
	}
</script>

<div class="scrim">
	<!-- A plain div, not <section>: svelte-check's a11y rule flags a landmark element (section)
	     being given an interactive role like "dialog" — see
	     https://svelte.dev/e/a11y_no_noninteractive_element_to_interactive_role -->
	<div
		class="sheet"
		role="dialog"
		aria-modal="true"
		aria-labelledby="conflict-review-heading"
		tabindex="-1"
		bind:this={dialogEl}
		onkeydown={onKeydown}
	>
		<h2 id="conflict-review-heading">Review conflicting edit</h2>

		{#if current}
			{#if isResurrectCandidate(current)}
				<p class="resurrect-note">
					<span aria-hidden="true">⚠</span> resurrected by a concurrent edit
				</p>
			{/if}

			<p class="merged-preview">{mergedPreview}</p>

			<DiffView mine={current.mine} theirs={current.theirs} />

			{#if error}
				<p class="error" role="alert">{error}</p>
			{/if}

			<div class="actions">
				<button type="button" disabled={resolving} onclick={() => resolve("mine")}>
					keep mine
				</button>
				<button type="button" disabled={resolving} onclick={() => resolve("theirs")}>
					keep theirs
				</button>
				<button type="button" disabled={resolving} onclick={() => resolve("merged")}>
					keep merged
				</button>
			</div>

			{#if flags.length > 1}
				<p class="remaining">{flags.length - 1} more after this</p>
			{/if}
		{/if}

		<button type="button" class="close" onclick={onClose}>Close</button>
	</div>
</div>

<style>
	.scrim {
		position: fixed;
		inset: 0;
		background: var(--color-overlay);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
	}

	.sheet {
		background: var(--color-bg-elevated);
		color: var(--color-text);
		border-radius: 8px;
		padding: 1.5rem;
		max-width: 32rem;
		width: 90%;
		max-height: 80vh;
		overflow-y: auto;
	}

	.merged-preview {
		font-family: ui-monospace, Menlo, monospace;
		background: var(--color-surface-muted);
		padding: 0.5rem;
		border-radius: 4px;
	}

	.resurrect-note {
		color: var(--color-warning-text);
		font-weight: 600;
	}

	.actions {
		display: flex;
		gap: 0.5rem;
		margin-top: 1rem;
	}

	.error {
		color: var(--color-danger);
	}

	.remaining {
		font-size: 0.85rem;
		color: var(--color-text-muted);
	}

	.close {
		margin-top: 1rem;
		background: transparent;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 0.25rem 0.75rem;
	}
</style>
