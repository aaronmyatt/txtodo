<script lang="ts">
	// Pin-on-top toggle (task desktop-always-on): an icon button in the header (task
	// desktop-ui-revamp), pressed while the window stays on top.
	import { pinned, setPinned } from "$lib/stores/pin";
	import Icon from "$lib/icons/Icon.svelte";

	function toggle(): void {
		void setPinned(!$pinned);
	}
</script>

<button
	type="button"
	class="icon-button pin-toggle"
	aria-pressed={$pinned}
	aria-label="Pin on top"
	onclick={toggle}
	title={$pinned ? "Always on top: on — click to unpin" : "Always on top: off — click to pin"}
>
	<Icon name="pin" />
</button>
