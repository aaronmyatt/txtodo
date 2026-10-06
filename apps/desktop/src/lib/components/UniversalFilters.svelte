<script lang="ts">
	// The Universal page's controls (task desktop-ui-revamp): group by, workspace chips (one always
	// stays on, the parent enforces it), show done, and the context chips for the rows in scope.
	import { GROUP_KINDS, type GroupKind } from "$lib/universal/rows";

	interface Props {
		group: GroupKind;
		showDone: boolean;
		context: string | null;
		workspaces: { id: string; label: string; open: number }[];
		hidden: Set<string>;
		contexts: [string, number][];
		onToggleWorkspace: (id: string) => void;
	}

	// $bindable: https://svelte.dev/docs/svelte/$bindable
	let {
		group = $bindable(),
		showDone = $bindable(),
		context = $bindable(),
		workspaces,
		hidden,
		contexts,
		onToggleWorkspace
	}: Props = $props();
</script>

<div class="controls">
	<div class="caps group-by">
		Group
		<div class="join" role="group" aria-label="Group by">
			{#each GROUP_KINDS as g (g.kind)}
				<button type="button" class:on={group === g.kind} aria-pressed={group === g.kind} onclick={() => (group = g.kind)}>
					{g.label}
				</button>
			{/each}
		</div>
	</div>
	<div class="chips" role="group" aria-label="Workspaces">
		{#each workspaces as w (w.id)}
			<button type="button" class="chip" class:on={!hidden.has(w.id)} aria-pressed={!hidden.has(w.id)} onclick={() => onToggleWorkspace(w.id)}>
				{w.label} <span class="n">{w.open}</span>
			</button>
		{/each}
	</div>
	<label class="done-toggle"><input type="checkbox" bind:checked={showDone} /> show done</label>
</div>
{#if contexts.length > 0}
	<div class="chips" role="group" aria-label="Filter by context">
		{#each contexts as [c, n] (c)}
			<button type="button" class="chip mono" class:on={context === c} aria-pressed={context === c} onclick={() => (context = context === c ? null : c)}>
				{c} <span class="n">{n}</span>
			</button>
		{/each}
	</div>
{/if}

<style>
	.controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 1rem;
		margin-top: 1.25rem;
	}

	.group-by {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.join {
		display: inline-flex;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		overflow: hidden;
	}

	.join button {
		padding: 0.15rem 0.6rem;
		border: 0;
		background: transparent;
		color: var(--color-base-content);
		font: inherit;
		font-size: 11px;
		letter-spacing: normal;
		text-transform: none;
		cursor: pointer;
	}

	.join button + button {
		border-left: 2px solid var(--color-base-content);
	}

	.join button.on,
	.chip.on {
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.375rem;
		margin-top: 0.75rem;
	}

	.controls .chips {
		margin-top: 0;
	}

	.chip {
		padding: 0.1rem 0.6rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: transparent;
		color: var(--color-base-content);
		font: inherit;
		font-size: 11px;
		font-weight: 700;
		cursor: pointer;
	}

	.n {
		opacity: 0.7;
	}

	.done-toggle {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin-left: auto;
		font-size: 0.75rem;
		cursor: pointer;
	}

	.done-toggle input {
		accent-color: var(--color-base-content);
	}

	.caps {
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}
</style>
