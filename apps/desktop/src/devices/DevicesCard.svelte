<script lang="ts">
	// Settings > Devices (task desktop-ui-revamp): pairing on the left, paired devices on the right
	// (stacked when narrow).
	import Devices from "./Devices.svelte";
	import DeviceList from "./DeviceList.svelte";
</script>

<div class="grid">
	<div><Devices /></div>
	<div><DeviceList /></div>
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: minmax(0, 19rem) minmax(0, 1fr);
		gap: 1.5rem;
	}

	@media (max-width: 56rem) {
		.grid {
			grid-template-columns: minmax(0, 1fr);
		}
	}
</style>
