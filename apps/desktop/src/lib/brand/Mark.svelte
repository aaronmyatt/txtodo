<script lang="ts">
	// The txtodo logo (assets/brand/tx-mark.svg). With `progress` (0..1) it becomes a meter: a faint
	// ghost of the whole mark, and its four strokes drawn in order over it (`mark.ts`). Without, the
	// plain mark. The first paint never animates (strokes start where they belong); later changes
	// ease in, unless the reader asked for reduced motion.
	// Ref: https://developer.mozilla.org/en-US/docs/Web/SVG/Attribute/pathLength
	import { onMount } from "svelte";
	import { MARK_PATHS, markOffsets } from "./mark";

	interface Props {
		/** 0..1 to draw the mark as a progress meter; omit for the plain mark. */
		progress?: number | null;
		/** What a screen reader hears, and the tooltip. */
		title?: string;
		class?: string;
	}

	let { progress = null, title = "txtodo", class: className = "" }: Props = $props();

	const offsets = $derived(progress === null ? null : markOffsets(progress));
	// Transitions only switch on after the first frame has painted.
	let painted = $state(false);
	onMount(() => {
		const frame = requestAnimationFrame(() => (painted = true));
		return () => cancelAnimationFrame(frame);
	});
</script>

<svg class="mark {className}" class:painted viewBox="0 0 100 64" role="img" aria-label={title}>
	<title>{title}</title>
	{#if offsets}
		<g class="ghost">
			{#each MARK_PATHS as d (d)}
				<path {d} />
			{/each}
		</g>
	{/if}
	<g>
		{#each MARK_PATHS as d, i (d)}
			{#if offsets}
				<path class="drawn" {d} pathLength="1" stroke-dasharray="1 2" stroke-dashoffset={offsets[i]} />
			{:else}
				<path {d} />
			{/if}
		{/each}
	</g>
</svg>

<style>
	.mark {
		display: block;
		overflow: visible;
		width: var(--mark-size, 2.5rem);
		height: auto;
	}

	path {
		fill: none;
		stroke: currentColor;
		stroke-width: 7;
		stroke-linecap: round;
		stroke-linejoin: round;
	}

	.ghost {
		opacity: 0.16;
	}

	.painted .drawn {
		transition: stroke-dashoffset 0.6s cubic-bezier(0.6, 0, 0.2, 1);
	}

	@media (prefers-reduced-motion: reduce) {
		.painted .drawn {
			transition: none;
		}
	}
</style>
