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

{#each $rejectedEdits as edit (`${edit.workspace}\n${edit.path}`)}
	<div class="banner tx-strip" role="alert">
		<span>Your edit to {edit.path} was not saved: {edit.error}</span>
		<button type="button" onclick={() => copyEdit(edit)}>Copy edit</button>
		<button type="button" class="dismiss" onclick={() => rejectedEdits.clear(edit.workspace, edit.path)} aria-label="Dismiss">×</button>
	</div>
{/each}

