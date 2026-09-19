<script lang="ts">
	// Shows every edit the daemon refused that FileView could not keep on screen (a path switch or
	// an unmount commits the outgoing buffer with no editor left to hold it) — see
	// $lib/stores/rejectedEdits.ts. "Copy edit" puts the refused text on the clipboard so nothing the
	// human typed is lost; dismissing only forgets this notice.
	// Ref: https://developer.mozilla.org/docs/Web/API/Clipboard/writeText
	import { rejectedEdits, type RejectedEdit } from "$lib/stores/rejectedEdits";

	async function copyEdit(edit: RejectedEdit) {
		try {
			await navigator.clipboard.writeText(edit.text);
		} catch (e) {
			console.error("copy of a rejected edit failed", e);
		}
	}
</script>

{#each $rejectedEdits as edit (edit.path)}
	<div class="banner" role="alert">
		<span>Your edit to {edit.path} was not saved: {edit.error}</span>
		<button type="button" onclick={() => copyEdit(edit)}>Copy edit</button>
		<button type="button" class="dismiss" onclick={() => rejectedEdits.clear(edit.path)} aria-label="Dismiss">×</button>
	</div>
{/each}

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
