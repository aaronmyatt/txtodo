<script lang="ts">
	// Conflict review sheet (plan M7; design §4.7 conflict table; plan §3.3 keyboard/a11y floor:
	// "popover traps focus"). A native `role="dialog"` with a hand-rolled focus trap — no new
	// dependency for it since the trap is ~15 lines (this task takes no new npm dependencies).
	// Ref (dialog pattern incl. focus trap + Escape): https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/
	import { onDestroy, onMount, tick } from "svelte";
	import { applyMutations, type DuplicateGroup } from "$lib/daemon";
	import { copiesLabel, keepMutations, type KeepChoice } from "$lib/stores/duplicates";

	// The file's duplicate groups (ADR 0032), first group first. Keeping one copy deletes the
	// others through an ordinary `applyMutations`, then `onGroupResolved` re-reads the groups (the
	// daemon derives them; nothing here to remove). Same-word review went with the todo.txt mirror
	// (ADR 0038).
	let {
		path,
		groups,
		onClose,
		onGroupResolved = async () => {}
	}: {
		path: string;
		groups: DuplicateGroup[];
		onClose: () => void;
		onGroupResolved?: () => Promise<void>;
	} = $props();

	const currentGroup = $derived(groups[0] as DuplicateGroup | undefined);

	let resolving = $state(false);
	let error = $state("");

	let dialogEl: HTMLElement | undefined;
	let previouslyFocused: HTMLElement | null = null;

	// Auto-close once every group this sheet was opened for has been resolved.
	$effect(() => {
		if (groups.length === 0) onClose();
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

	/** Keeps one copy of the current group: one `applyMutations` of deletes by task id. */
	async function keep(choice: KeepChoice) {
		if (!currentGroup || resolving) return;
		resolving = true;
		error = "";
		try {
			await applyMutations(path, keepMutations(currentGroup, choice));
			await onGroupResolved();
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
		<h2 id="conflict-review-heading">Review a duplicate line</h2>

		{#if currentGroup}
			<p>
				The same text is on {copiesLabel(currentGroup)}. Keep one copy, or edit one so they
				differ: the buffer stays editable.
			</p>

			{#if error}
				<p class="error" role="alert">{error}</p>
			{/if}

			<div class="actions">
				<button type="button" disabled={resolving} onclick={() => keep("newest")}>
					keep newest
				</button>
				<button type="button" disabled={resolving} onclick={() => keep("oldest")}>
					keep oldest
				</button>
			</div>

			{#if groups.length > 1}
				<p class="remaining">{groups.length - 1} more after this</p>
			{/if}
		{/if}

		<button type="button" class="close" onclick={onClose}>Close</button>
	</div>
</div>

<style>
	/* The mockup's dialog: a 2px ink outline, round box, flat; pill buttons, ink first. */
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
		background: var(--color-base-100);
		color: var(--color-base-content);
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		padding: 1.5rem;
		max-width: 32rem;
		width: 90%;
		max-height: 80vh;
		overflow-y: auto;
		font-size: 0.875rem;
	}

	h2 {
		margin: 0 0 1rem;
		font-family: var(--font-brand);
		font-size: 1rem;
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 1rem;
	}

	.actions button,
	.close {
		padding: 0.3rem 0.9rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		cursor: pointer;
	}

	.actions button:first-child {
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.actions button:disabled {
		opacity: 0.5;
		cursor: default;
	}

	.error {
		color: var(--color-error);
	}

	.remaining {
		font-size: 0.75rem;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.close {
		margin-top: 1rem;
		border-color: color-mix(in srgb, var(--color-base-content) 30%, transparent);
	}
</style>
