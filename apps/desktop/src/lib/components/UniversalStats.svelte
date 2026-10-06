<script lang="ts">
	// The Universal stat strip (task desktop-ui-revamp): overdue, due this week, open, done.
	import type { Stats } from "$lib/universal/rows";

	let { totals }: { totals: Stats } = $props();
</script>

<div class="stats">
	<div class="stat"><b class:overdue={totals.overdue > 0}>{totals.overdue}</b><span class="caps">Overdue</span></div>
	<div class="stat"><b class:soon={totals.soon > 0}>{totals.soon}</b><span class="caps">Due this week</span></div>
	<!-- "t open" / "x done" keep the logo's two letters lower case. -->
	<div class="stat"><b>{totals.open}</b><span>t open</span></div>
	<div class="stat"><b>{totals.done}</b><span>x done</span></div>
</div>

<style>
	.stats {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 0.75rem;
		margin-top: 1.25rem;
	}

	.stat {
		display: flex;
		flex-direction: column;
		padding: 0.75rem 1rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 20%, transparent);
		border-radius: var(--radius-box);
	}

	.stat b {
		font-size: 1.875rem;
		letter-spacing: -0.05em;
		font-variant-numeric: tabular-nums;
	}

	.stat > span:not(.caps) {
		font-size: 11px;
		font-weight: 700;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.caps {
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.overdue {
		color: var(--color-error, #dc2626);
	}

	.soon {
		color: var(--color-warning, #d97706);
	}

	@media (max-width: 40rem) {
		.stats {
			grid-template-columns: repeat(2, 1fr);
		}
	}
</style>
