<script lang="ts">
	// The detail as a bottom panel (decision 2, 2026-09-27), shared by Tasks and Universal (2026-10-06
	// feedback: opening a Universal row's detail must not leave the page). 55% of the height under a
	// 2px ink rule, scrolling on its own, with a close button; Esc outside an editor closes it too.
	import Icon from "$lib/icons/Icon.svelte";
	import { isTypingTarget } from "$lib/keys";
	import type { DetailParams } from "$lib/types";
	import DetailView from "./DetailView.svelte";

	interface Props {
		steps: DetailParams[];
		rootPath: string;
		onNavigateInto: (params: DetailParams) => void;
		onNavigateToLevel: (stackLength: number) => void;
	}

	let { steps, rootPath, onNavigateInto, onNavigateToLevel }: Props = $props();

	/** Esc closes the panel, unless an editor or field took it (it discards there). */
	function onWindowKeydown(e: KeyboardEvent) {
		if (e.key !== "Escape" || e.defaultPrevented || isTypingTarget(e.target)) return;
		onNavigateToLevel(0);
	}
</script>

<svelte:window onkeydown={onWindowKeydown} />

<section class="detail-panel" aria-label="Task detail">
	<button type="button" class="close-detail" aria-label="Close detail" onclick={() => onNavigateToLevel(0)}>
		<Icon name="x" />
	</button>
	<DetailView {steps} {rootPath} {onNavigateInto} {onNavigateToLevel} />
</section>

<style>
	.detail-panel {
		position: relative;
		flex-shrink: 0;
		height: 55%;
		overflow-y: auto;
		border-top: 2px solid var(--color-base-content);
		background: var(--color-base-100);
	}

	.close-detail {
		position: absolute;
		top: 0.75rem;
		right: 0.75rem;
		z-index: 1;
		display: inline-flex;
		padding: 0.3rem;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		cursor: pointer;
		--icon-size: 1rem;
	}

	.close-detail:hover {
		background: var(--color-base-200);
	}
</style>
