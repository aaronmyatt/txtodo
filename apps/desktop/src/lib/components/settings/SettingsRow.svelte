<script lang="ts">
	// One Settings row (task desktop-ui-revamp, the mockup's `row()`): label and a short plain
	// description on the left, the control on the right, a 10% rule between rows.
	import type { Snippet } from "svelte";

	interface Props {
		label: string;
		description?: string;
		/** The control's id, so the label clicks through to it. */
		for?: string;
		children?: Snippet;
	}

	let { label, description = "", for: forId, children }: Props = $props();
</script>

<div class="row">
	<div class="text">
		{#if forId}<label for={forId}>{label}</label>{:else}<span class="label">{label}</span>{/if}
		{#if description}<p>{description}</p>{/if}
	</div>
	{#if children}<div class="control">{@render children()}</div>{/if}
</div>

<style>
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 1.5rem;
		padding: 0.75rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	/* first-of-type: the card's h2 comes first. */
	.row:first-of-type {
		padding-top: 0;
		border-top: 0;
	}

	.row:last-child {
		padding-bottom: 0;
	}

	.text {
		flex: 1 1 16rem;
		min-width: 0;
	}

	label,
	.label {
		font-size: 0.875rem;
		font-weight: 700;
	}

	p {
		margin: 0.125rem 0 0;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
		line-height: 1.6;
	}

	.control {
		flex-shrink: 0;
	}
</style>
