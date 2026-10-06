<script lang="ts">
	// Settings (task desktop-ui-revamp): one page of cards with a sticky section nav. The nav follows
	// the scroll (scroll-spy, `activeSection`), the header search hides cards that miss a term
	// (`sectionMatches`), and a deep link (`/settings#tokens`) lands on its card.
	import { onMount, tick, type Snippet } from "svelte";
	import { searchQuery, settingsHits } from "$lib/stores/search";
	import { activeSection, SECTIONS, sectionMatches } from "$lib/settings/sections";
	import SettingsCard from "$lib/components/SettingsCard.svelte";
	import GeneralSettings from "$lib/components/settings/GeneralSettings.svelte";
	import AppearanceSettings from "$lib/components/settings/AppearanceSettings.svelte";
	import WorkspacesSettings from "$lib/components/settings/WorkspacesSettings.svelte";
	import ShortcutsSettings from "$lib/components/settings/ShortcutsSettings.svelte";
	import DevicesCard from "../../devices/DevicesCard.svelte";
	import Tokens from "../../devices/Tokens.svelte";
	import ActivityFeed from "../../devices/ActivityFeed.svelte";

	/** The cards built so far, by section id; a section without one is not shown yet. */
	const BODIES: Record<string, { body: Snippet; headless?: boolean }> = {
		general: { body: general },
		appearance: { body: appearance },
		workspaces: { body: workspaces },
		shortcuts: { body: shortcuts },
		devices: { body: devices },
		tokens: { body: tokens, headless: true },
		activity: { body: activity, headless: true }
	};

	const shown = $derived(SECTIONS.filter((s) => BODIES[s.id] && sectionMatches(s, $searchQuery)));
	$effect(() => settingsHits.set(shown.length));

	let scroller: HTMLElement | undefined = $state();
	let active = $state<string | null>(null);

	/** Scroll-spy: the card whose top passed a line a third down the view. */
	function spy() {
		if (!scroller) return;
		const top = scroller.getBoundingClientRect().top;
		const tops = shown.flatMap((s) => {
			const el = document.getElementById(s.id);
			return el ? [{ id: s.id, top: el.getBoundingClientRect().top - top }] : [];
		});
		const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4;
		active = activeSection(tops, scroller.clientHeight / 3, atEnd);
	}

	function go(id: string) {
		// scrollIntoView: https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollIntoView
		document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
		active = id;
		history.replaceState(history.state, "", `#${id}`);
	}

	$effect(() => {
		void shown;
		void tick().then(spy);
	});

	let cardsEl: HTMLElement | undefined = $state();

	onMount(() => {
		const id = location.hash.slice(1);
		if (id) void tick().then(() => document.getElementById(id)?.scrollIntoView({ block: "start" }));
		// Cards grow as their data arrives without any scroll event, so the spy re-reads on resize:
		// https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver
		const resize = new ResizeObserver(() => spy());
		if (cardsEl) resize.observe(cardsEl);
		return () => resize.disconnect();
	});
</script>

{#snippet general()}<GeneralSettings />{/snippet}
{#snippet appearance()}<AppearanceSettings />{/snippet}
{#snippet workspaces()}<WorkspacesSettings />{/snippet}
{#snippet shortcuts()}<ShortcutsSettings />{/snippet}
{#snippet devices()}<DevicesCard />{/snippet}
{#snippet tokens()}<Tokens />{/snippet}
{#snippet activity()}<ActivityFeed />{/snippet}

<main class="settings" bind:this={scroller} onscroll={spy}>
	<div class="page">
		<nav aria-label="Settings sections">
			<h1>Settings</h1>
			{#each shown as s (s.id)}
				<button type="button" class:on={active === s.id} aria-current={active === s.id ? "true" : undefined} onclick={() => go(s.id)}>
					{s.title}
				</button>
			{/each}
		</nav>
		<div class="cards" bind:this={cardsEl}>
			{#each shown as s (s.id)}
				<SettingsCard section={s} headless={BODIES[s.id].headless}>
					{@render BODIES[s.id].body()}
				</SettingsCard>
			{/each}
			{#if shown.length === 0}
				<p class="none">No settings match that search.</p>
			{/if}
		</div>
	</div>
</main>

<style>
	.settings {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
	}

	.page {
		display: flex;
		gap: 2rem;
		max-width: 72rem;
		margin: 0 auto;
		padding: 1.5rem 1.25rem 3rem;
	}

	/* The mockup's sticky nav: the title, then one pill per card, the current one inverse. */
	nav {
		position: sticky;
		top: 1.5rem;
		display: flex;
		flex-direction: column;
		flex-shrink: 0;
		gap: 0.25rem;
		width: 11rem;
		height: fit-content;
	}

	h1 {
		margin: 0 0 0.75rem;
		font-size: 1.5rem;
		letter-spacing: -0.05em;
	}

	nav button {
		padding: 0.375rem 0.75rem;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font: inherit;
		font-size: 0.75rem;
		font-weight: 700;
		text-align: left;
		cursor: pointer;
	}

	nav button:hover {
		background: color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	nav button.on {
		background: var(--color-base-content);
		color: var(--color-base-100);
	}

	.cards {
		display: flex;
		flex: 1;
		flex-direction: column;
		gap: 1.5rem;
		min-width: 0;
	}

	.none {
		padding: 2rem;
		border: 2px dashed color-mix(in srgb, var(--color-base-content) 30%, transparent);
		border-radius: var(--radius-box);
		color: color-mix(in srgb, var(--color-base-content) 70%, transparent);
		font-family: var(--font-sans);
		font-size: 0.875rem;
		text-align: center;
	}

	/* Narrow windows: the nav would crowd the cards, so it goes (the title stays). */
	@media (max-width: 48rem) {
		nav button {
			display: none;
		}

		.page {
			flex-direction: column;
			gap: 0;
		}
	}
</style>
