<script lang="ts">
	// Settings > Shortcuts (task desktop-ui-revamp): the shared key table. The global hotkey is
	// fixed in `quick_add.rs` for now, so "Change…" stays disabled until it is configurable.
	import KeyTable from "../KeyTable.svelte";
</script>

<KeyTable />
<p class="note">
	The global hotkey is fixed for now.
	<button type="button" disabled title="Not configurable yet">Change…</button>
</p>

<style>
	.note {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin: 1rem 0 0;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	button {
		padding: 0.15rem 0.7rem;
		border: 2px solid currentColor;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		opacity: 0.5;
	}
</style>
