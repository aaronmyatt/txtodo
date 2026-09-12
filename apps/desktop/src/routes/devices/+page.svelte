<script lang="ts">
	// Devices/agents screen route (tasks/desktop-devices-screen): the three panes are
	// self-contained (each talks to the daemon over its own Tauri commands), so this route is
	// just the mount point + a back link.
	import Devices from "../../devices/Devices.svelte";
	import Tokens from "../../devices/Tokens.svelte";
	import ActivityFeed from "../../devices/ActivityFeed.svelte";
</script>

<main class="devices-page">
	<a href="/">‹ Back</a>
	<h1>Devices &amp; agents</h1>
	<Devices />
	<Tokens />
	<ActivityFeed />
</main>

<style>
	.devices-page {
		padding: 2rem;
		display: flex;
		flex-direction: column;
		gap: 2rem;
	}
</style>
