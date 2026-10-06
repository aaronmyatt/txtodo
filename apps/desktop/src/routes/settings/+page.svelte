<script lang="ts">
	// Settings (task desktop-ui-revamp): one page of cards, each a section with an id so a deep link
	// (`/settings#tokens`) lands on it. The cards that exist today are the old Devices & agents
	// screen's three panes; General, Appearance, Workspaces and Shortcuts join with their lines, as
	// do the sticky section nav and the card filter.
	import Devices from "../../devices/Devices.svelte";
	import Tokens from "../../devices/Tokens.svelte";
	import ActivityFeed from "../../devices/ActivityFeed.svelte";
</script>

<main class="settings">
	<h1>Settings</h1>
	<section id="devices" aria-label="Devices">
		<Devices />
	</section>
	<section id="tokens" aria-label="Tokens">
		<Tokens />
	</section>
	<section id="activity" aria-label="Activity">
		<ActivityFeed />
	</section>
</main>

<style>
	.settings {
		display: flex;
		flex-direction: column;
		gap: 2rem;
		padding: 2rem;
	}

	h1 {
		margin: 0;
	}

</style>
