<script lang="ts">
	// One Settings card (task desktop-ui-revamp): a section with the id deep links and the nav use.
	import type { Snippet } from "svelte";
	import type { Section } from "$lib/settings/sections";

	let { section, children }: { section: Section; children: Snippet } = $props();
</script>

<section id={section.id} class="card" aria-label={section.title}>
	<h2>{section.title}</h2>
	{@render children()}
</section>

<style>
	.card {
		scroll-margin-top: 1rem;
		padding: 1.25rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 20%, transparent);
		border-radius: var(--radius-box);
	}

	h2 {
		margin: 0 0 1rem;
		font-size: 1.125rem;
		letter-spacing: -0.025em;
	}
</style>
