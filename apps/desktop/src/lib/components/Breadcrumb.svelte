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

	// Only the entry level ever carries `workspaceRoot` (MainView's own `$currentWorkspaceRoot`
	// effect sets it exclusively on `steps[0]` when the universal view opened this stack) — every
	// deeper level belongs to that same workspace, so one leading crumb covers the whole stack.
	const project = $derived(steps[0]?.workspaceRoot);

	/** Last path segment of an absolute root, for a compact crumb; falls back to the full path for
	 * a bare name with no separator. */
	function projectLabel(root: string): string {
		return (
			root
				.split(/[/\\]/)
				.filter(Boolean)
				.pop() ?? root
		);
	}
</script>

<nav aria-label="Breadcrumb" class="breadcrumb">
	{#if project}
		<span class="crumb project" title={project}>{projectLabel(project)}</span>
		<span class="sep" aria-hidden="true">&rsaquo;</span>
	{/if}
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

	.crumb.project {
		font-weight: 600;
		text-decoration: none;
		cursor: default;
	}

	.sep {
		opacity: 0.5;
	}
</style>
