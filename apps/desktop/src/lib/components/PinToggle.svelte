<script lang="ts">
	// Pin-on-top toggle (task desktop-always-on), styled to match ThemeToggle.svelte's existing
	// plain-button toolbar look rather than introducing a new control style for one more
	// low-frequency top-nav action.
	import { pinned, setPinned } from "$lib/stores/pin";

	function toggle(): void {
		void setPinned(!$pinned);
	}
</script>

<button
	type="button"
	class="pin-toggle"
	class:active={$pinned}
	onclick={toggle}
	title={$pinned ? "Always on top: on — click to unpin" : "Always on top: off — click to pin"}
>
	<span aria-hidden="true">📌</span>
	{$pinned ? "Pinned" : "Pin"}
</button>

<style>
	.pin-toggle {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.85rem;
		padding: 0.25rem 0.6rem;
		border-radius: 999px;
		border: 1px solid var(--color-border);
		background: var(--color-surface-muted);
		color: var(--color-text);
		cursor: pointer;
	}

	.pin-toggle:hover {
		background: var(--color-surface);
	}

	.pin-toggle.active {
		border-color: var(--color-accent, var(--color-border));
		background: var(--color-surface);
	}
</style>
