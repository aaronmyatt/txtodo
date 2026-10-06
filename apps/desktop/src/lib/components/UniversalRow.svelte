<script lang="ts">
	// One Universal row (task desktop-ui-revamp): the description in the editor's token colours,
	// then the due badge, the `ref:` sub-list's n/m, and the workspace tile.
	import type { UniversalTask } from "$lib/daemon";
	import { hitRanges, isActiveQuery } from "$lib/todotxt/query";
	import { markHits, segments } from "$lib/todotxt/segments";
	import Icon from "$lib/icons/Icon.svelte";
	import type { GroupKind, Row } from "$lib/universal/rows";

	interface Props {
		row: Row;
		group: GroupKind;
		onOpen: (task: UniversalTask) => void;
		onToggle: (row: Row) => void;
		/** The roving tab stop: only this row's open button is in the tab order. */
		active: boolean;
		/** The header search, whose hits are marked in the description. */
		query: string;
	}

	let { row, group, onOpen, onToggle, active, query }: Props = $props();

	const segs = $derived.by(() => {
		const d = row.task.description;
		return isActiveQuery(query) ? markHits(segments(d), hitRanges(d, query)) : segments(d);
	});
</script>

<div class="row" class:done={row.task.done}>
	<!-- The round mark: done fills it with ink. Out of the tab order; `x` on a row does the same. -->
	<button
		type="button"
		class="mark-btn"
		class:is-done={row.task.done}
		tabindex="-1"
		aria-label={row.task.done ? "Mark not done" : "Mark done"}
		onclick={() => onToggle(row)}><Icon name="x" /></button
	>
	<button type="button" class="open" data-key={row.key} tabindex={active ? 0 : -1} onclick={() => onOpen(row.task)}>
		{#if group !== "priority" && row.task.priority}
			<span class="tok-priority">({row.task.priority})</span>
		{/if}
		<span class="body">
			{#each segs as s, i (i)}<span class={s.cls} class:hit={s.hit}>{s.text}</span>{/each}
		</span>
	</button>
	{#if row.due}
		<span class="due {row.due.tone}">{row.due.text}</span>
	{/if}
	{#if row.task.ref_progress}
		<span class="ref" title="sub-list progress">{row.task.ref_progress.done}/{row.task.ref_progress.total}</span>
	{/if}
	{#if group !== "workspace"}
		<span class="tile" title={row.task.workspace_root}>{row.ws.slice(0, 2)}</span>
	{/if}
</div>

<style>
	/* The mockup's .tx-mark-btn. */
	.mark-btn {
		display: grid;
		flex-shrink: 0;
		place-items: center;
		width: 1.2rem;
		height: 1.2rem;
		padding: 0;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 50%, transparent);
		border-radius: 999px;
		background: transparent;
		color: transparent;
		cursor: pointer;
		transition:
			border-color 0.12s,
			background-color 0.12s,
			color 0.12s;
		--icon-size: 0.7rem;
	}

	.mark-btn:hover {
		border-color: var(--color-base-content);
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}

	.mark-btn.is-done {
		border-color: var(--color-base-content);
		background: var(--color-base-content);
		color: var(--color-base-100);
	}

	.row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.375rem 0.5rem;
		border-radius: var(--radius-field);
	}

	/* A visible 2px ring on the whole row while its open button has the keyboard.
	   :has() https://developer.mozilla.org/en-US/docs/Web/CSS/:has */
	.row:has(.open:focus-visible) {
		outline: 2px solid var(--color-base-content);
		outline-offset: -2px;
	}

	.open:focus-visible {
		outline: none;
	}

	/* A hit: the inverse tile in miniature, as in the buffer (search.ts's .cm-hit). */
	.hit {
		border-radius: 0.2em;
		background: var(--color-base-content);
		color: var(--color-base-100) !important;
	}

	.row:hover {
		background: var(--color-base-200);
	}

	.row.done {
		opacity: 0.55;
	}

	.row.done .body {
		text-decoration: line-through;
	}

	.open {
		display: flex;
		flex: 1;
		align-items: center;
		gap: 0.75rem;
		min-width: 0;
		padding: 0;
		border: 0;
		background: transparent;
		color: inherit;
		font: inherit;
		font-size: 0.875rem;
		text-align: left;
		cursor: pointer;
	}

	.body {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.due,
	.ref {
		flex-shrink: 0;
		padding: 0 0.5rem;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 700;
	}

	.due.overdue {
		background: var(--color-error, #dc2626);
		color: var(--color-error-content, #fff);
	}

	.due.today {
		background: var(--color-warning, #d97706);
		color: var(--color-warning-content, #000);
	}

	.due.soon {
		background: color-mix(in srgb, var(--color-warning, #d97706) 20%, transparent);
		color: var(--color-base-content);
	}

	.due.later {
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
	}

	.ref {
		border: 2px solid color-mix(in srgb, var(--color-base-content) 30%, transparent);
	}

	.tile {
		display: grid;
		flex-shrink: 0;
		place-items: center;
		width: 1.5rem;
		height: 1.5rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 40%, transparent);
		border-radius: 0.5rem;
		font-size: 10px;
		font-weight: 700;
		letter-spacing: -0.05em;
	}

	/* The editor's token colours (todotxtLanguage.ts), here outside an editor; scoped to the row. */
	.row :global(.tok-priority) {
		color: var(--tok-priority);
		font-weight: 700;
	}

	.row :global(.tok-date) {
		color: var(--tok-date);
	}

	.row :global(.tok-project) {
		color: var(--tok-project);
	}

	.row :global(.tok-context) {
		color: var(--tok-context);
	}

	.row :global(.tok-tag-key) {
		color: var(--tok-tag-key);
		font-weight: 600;
	}

	.row :global(.tok-tag-value) {
		color: var(--tok-tag-value);
	}

	.row :global(.tok-id-tag) {
		color: var(--tok-id-tag);
	}
</style>
