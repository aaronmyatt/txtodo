<script lang="ts">
	// Breadcrumb for the detail view (tasks/desktop-detail-view, plan §3.2): "todo.txt › 2 ›
	// q4-roadmap/todo.txt › 3" for a two-level-deep nested detail view. Each `steps[i]` is one open
	// detail level — `{file, line}` of that level's pinned parent (see $lib/types.ts) — so the
	// breadcrumb is just `steps` rendered as alternating file/line crumbs, plus a leading "Home"
	// crumb back to the root file view (steps === []).
	//
	// Structural only: no routing, no history API. `onNavigate` receives the *stack length* to
	// truncate to (0 = home; i+1 = "reopen exactly through step i"), matching
	// `MainView.svelte`'s `detail` stack so a click here is just `detail = detail.slice(0, n)`.
	import type { BreadcrumbStep } from "$lib/types";

	let { steps, onNavigate }: { steps: BreadcrumbStep[]; onNavigate: (stackLength: number) => void } =
		$props();
</script>

<nav aria-label="Breadcrumb" class="breadcrumb">
	<button type="button" class="crumb" onclick={() => onNavigate(0)}>Home</button>
	{#each steps as step, i (i)}
		<span class="sep" aria-hidden="true">&rsaquo;</span>
		<button
			type="button"
			class="crumb"
			aria-current={i === steps.length - 1 ? "page" : undefined}
			onclick={() => onNavigate(i + 1)}
		>
			{step.file}
		</button>
		<span class="sep" aria-hidden="true">&rsaquo;</span>
		<span class="crumb-line">{step.line}</span>
	{/each}
</nav>

<style>
	.breadcrumb {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.25rem;
		font-size: 0.9rem;
	}

	.crumb {
		background: transparent;
		border: none;
		padding: 0.1rem 0.25rem;
		color: inherit;
		cursor: pointer;
		text-decoration: underline;
		text-underline-offset: 2px;
	}

	.crumb[aria-current="page"] {
		font-weight: 600;
		text-decoration: none;
	}

	.crumb-line {
		opacity: 0.75;
	}

	.sep {
		opacity: 0.5;
	}
</style>
