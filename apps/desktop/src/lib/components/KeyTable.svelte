<script lang="ts">
	// The keys reference (`$lib/keyTable.ts`), shared by Settings > Shortcuts and the Help page. The
	// hotkey comes from the Rust side; until it answers (or outside Tauri without the mock) its row
	// is simply absent.
	import { onMount } from "svelte";
	import { quickAddHotkey } from "$lib/daemon";
	import { keyGroups, keyLabel } from "$lib/keyTable";

	let hotkey = $state<string[]>([]);
	const groups = $derived(keyGroups(hotkey));
	// navigator.platform is deprecated but still the plainest "is this a Mac" read in WebKit:
	// https://developer.mozilla.org/en-US/docs/Web/API/Navigator/platform
	const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform || navigator.userAgent);

	onMount(() => {
		quickAddHotkey()
			.then((h) => (hotkey = h))
			.catch(() => {});
	});
</script>

<table>
	<tbody>
		{#each groups as g (g.title)}
			<tr><th colspan="2" scope="colgroup">{g.title}</th></tr>
			{#each g.rows as row (row.title)}
				<tr>
					<td class="keys">
						{#each row.keys as alt, a (a)}
							{#if a > 0}<span class="sep">/</span>{/if}
							{#each alt as press, p (p)}
								{#if p > 0}<span class="then">then</span>{/if}
								{#each press as k, i (i)}<kbd>{keyLabel(k, mac)}</kbd>{/each}
							{/each}
						{/each}
					</td>
					<td class="what">{row.title}</td>
				</tr>
			{/each}
		{/each}
	</tbody>
</table>

<style>
	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		padding: 1rem 0 0.25rem;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.05em;
		text-align: left;
		text-transform: uppercase;
	}

	tr:first-child th {
		padding-top: 0;
	}

	td {
		padding: 0.3rem 0;
		vertical-align: middle;
	}

	.keys {
		width: 13rem;
		white-space: nowrap;
	}

	kbd {
		display: inline-block;
		min-width: 1.4rem;
		margin-right: 0.2rem;
		padding: 0.05rem 0.35rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 35%, transparent);
		border-bottom-width: 3px;
		border-radius: 0.4rem;
		font: inherit;
		font-size: 0.75rem;
		font-weight: 700;
		text-align: center;
	}

	.sep,
	.then {
		margin: 0 0.35rem 0 0.1rem;
		color: color-mix(in srgb, var(--color-base-content) 55%, transparent);
		font-size: 0.6875rem;
	}

	.what {
		font-family: var(--font-sans);
		font-size: 0.875rem;
	}
</style>
