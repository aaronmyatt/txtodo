<script lang="ts">
	// The toasts from `$lib/stores/toast.ts`, bottom right above the prompt bar. A polite live
	// region, so a screen reader hears "Completed in work" without losing its place:
	// https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Attributes/aria-live
	import { dismissToast, toasts } from "$lib/stores/toast";
</script>

<div class="toasts" aria-live="polite">
	{#each $toasts as t (t.id)}
		<div class="toast">
			<span>{t.message}</span>
			{#if t.undo}
				<button
					type="button"
					onclick={() => {
						t.undo?.();
						dismissToast(t.id);
					}}>Undo</button
				>
			{/if}
		</div>
	{/each}
</div>

<style>
	.toasts {
		position: fixed;
		right: 1rem;
		bottom: 8.5rem;
		z-index: 50;
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 0.5rem;
		pointer-events: none;
	}

	.toast {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.5rem 0.75rem;
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
		font-size: 0.75rem;
		pointer-events: auto;
	}

	button {
		padding: 0.1rem 0.6rem;
		border: 0;
		border-radius: 999px;
		background: var(--color-primary);
		color: var(--color-primary-content);
		font: inherit;
		font-weight: 700;
		cursor: pointer;
	}
</style>
