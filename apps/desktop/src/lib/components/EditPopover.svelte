<script lang="ts">
	// TODO(placeholder): the real edit popover (plan §3.2: token chips, footer with op-log
	// attribution, strict/lenient validation) is being built on a parallel branch and will
	// overwrite this file on merge. This stub only matches the prop contract in
	// tasks/desktop-main-view/todo.txt so `FileView.svelte`/`MainView.svelte` compile and the
	// click-to-edit affordance is exercisable meanwhile: a single-line input, Enter/Save commits,
	// Esc/Cancel discards. No token chips, no validation, no op-log footer.
	import type { TaskRef } from "$lib/daemon";

	interface Props {
		initialLine: string;
		taskRef: TaskRef;
		anchor: HTMLElement | null;
		onSave: (newLine: string) => void;
		onCancel: () => void;
	}

	let { initialLine, taskRef, anchor, onSave, onCancel }: Props = $props();

	// svelte-ignore state_referenced_locally -- intentional one-shot capture: the buffer only
	// tracks the prop at popover-open time, edits happen locally until Save/Cancel.
	let value = $state(initialLine);
	let style = $state("");

	// Anchors the stub popover under the clicked line, same as the real one will.
	$effect(() => {
		if (!anchor) return;
		const rect = anchor.getBoundingClientRect();
		style = `top: ${rect.bottom + window.scrollY}px; left: ${rect.left + window.scrollX}px;`;
	});

	function save() {
		onSave(value);
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === "Enter") save();
		else if (e.key === "Escape") onCancel();
	}
</script>

<div class="popover" {style} role="dialog" aria-label={`Edit line ${taskRef.line_number}`}>
	<input type="text" bind:value onkeydown={onKeydown} />
	<div class="actions">
		<button onclick={save}>Save</button>
		<button onclick={onCancel}>Cancel</button>
	</div>
</div>

<style>
	.popover {
		position: absolute;
		z-index: 10;
		display: flex;
		gap: 0.5rem;
		align-items: center;
		background: var(--cm-popover-bg, #fff);
		border: 1px solid #d1d5db;
		border-radius: 6px;
		padding: 0.4rem 0.5rem;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.12);
	}

	input {
		flex: 1;
		font: inherit;
		min-width: 20rem;
	}
</style>
