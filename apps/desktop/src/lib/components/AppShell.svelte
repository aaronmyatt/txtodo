<script lang="ts">
	// The window's frame on every page (task desktop-ui-revamp, the c2 mockup's header): the mark and
	// wordmark, the workspace menu, the header search, the Tasks / Universal / Settings switch and
	// the pin / theme / help buttons, over the page's own content. Replaces the per-page top bars
	// MainView, the universal, devices and help pages each drew.
	// Ref: https://svelte.dev/docs/kit/$app-state#page
	import type { Snippet } from "svelte";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import Mark from "$lib/brand/Mark.svelte";
	import Icon from "$lib/icons/Icon.svelte";
	import type { IconName } from "$lib/icons/icons";
	import { Chords, isTypingTarget, keyName } from "$lib/keys";
	import { searchQuery, searchStep, universalHits } from "$lib/stores/search";
	import { editorStatus, SAVE_LABEL } from "$lib/stores/editorStatus";
	import PinToggle from "./PinToggle.svelte";
	import PromptBar from "./PromptBar.svelte";
	import Toasts from "./Toasts.svelte";
	import ThemeToggle from "./ThemeToggle.svelte";
	import WorkspaceSwitcher from "./WorkspaceSwitcher.svelte";

	let { children }: { children: Snippet } = $props();

	/** The three views; `match` says which paths count as being on it. */
	const NAV: { href: string; label: string; icon: IconName; match: (path: string) => boolean }[] = [
		{ href: "/", label: "Tasks", icon: "list", match: (p) => p === "/" },
		{ href: "/universal", label: "Universal", icon: "stack", match: (p) => p.startsWith("/universal") },
		{ href: "/settings", label: "Settings", icon: "sliders", match: (p) => p.startsWith("/settings") }
	];

	const path = $derived(page.url.pathname);

	// The shell's keys (`$lib/keys.ts`): `/` focuses the search, `g t|u|s` go to a view, `?` opens
	// Help. Never while the reader is typing in a field, an editor or a dialog.
	// Ref: https://svelte.dev/docs/kit/$app-navigation#goto
	let searchEl: HTMLInputElement | undefined = $state();
	const chords = new Chords();
	const ROUTES: Record<string, string> = {
		"nav.tasks": "/",
		"nav.universal": "/universal",
		"nav.settings": "/settings",
		"nav.help": "/help"
	};

	// The footer's key hints for the view in front; `Mod` is ⌘ on macOS, Ctrl elsewhere.
	const MOD = typeof navigator !== "undefined" && /Mac/.test(navigator.platform) ? "⌘" : "Ctrl+";
	const hints = $derived(
		path === "/"
			? [
					[`${MOD}S`, "save"],
					["esc", "discard"],
					[`${MOD}↵`, "open"],
					["/", "search"]
				]
			: path.startsWith("/help")
				? [["g t", "tasks"]]
				: [
						["/", "search"],
						["g t", "tasks"]
					]
	);

	/** Keys in the search field: Enter / Shift+Enter step through the hits (focus stays here),
	 * Esc clears, and a second Esc leaves the field. */
	function onSearchKeydown(e: KeyboardEvent) {
		if (e.key === "Enter") {
			e.preventDefault();
			searchStep.update((s) => ({ dir: e.shiftKey ? -1 : 1, n: s.n + 1 }));
		} else if (e.key === "Escape") {
			e.preventDefault();
			if ($searchQuery !== "") $searchQuery = "";
			else searchEl?.blur();
		}
	}

	// The count pill: `current/hits` once stepping, else the hit count; on Universal, its rows.
	const pill = $derived.by(() => {
		if ($searchQuery.trim() === "") return "";
		if (path === "/universal") return `${$universalHits}`;
		if (path !== "/") return "";
		const { hits, index } = $editorStatus.search;
		return index > 0 ? `${index}/${hits}` : `${hits}`;
	});

	function onKeydown(e: KeyboardEvent) {
		if (e.defaultPrevented || isTypingTarget(e.target)) return;
		const name = keyName(e);
		if (!name) return;
		const hit = chords.resolve(name, e.timeStamp);
		if (hit.kind === "none") return;
		e.preventDefault();
		if (hit.kind !== "command") return;
		if (hit.id === "search.focus") searchEl?.focus();
		else if (ROUTES[hit.id]) void goto(ROUTES[hit.id]);
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div class="shell">
	<header class="header">
		<a href="/" class="brand" aria-label="txtodo, Tasks">
			<Mark class="brand-mark" />
			<span class="wordmark">txtodo</span>
		</a>
		<WorkspaceSwitcher />

		<label class="search">
			<Icon name="search" class="search-icon" />
			<input
				type="text"
				placeholder="Search"
				aria-label="Search"
				autocomplete="off"
				spellcheck="false"
				bind:value={$searchQuery}
				bind:this={searchEl}
				onkeydown={onSearchKeydown}
			/>
			{#if pill}
				<span class="pill" role="status" aria-live="polite" aria-label="Matching lines">{pill}</span>
			{/if}
			<kbd>/</kbd>
		</label>

		<nav class="views" aria-label="Views">
			{#each NAV as item (item.href)}
				<a href={item.href} class="view" aria-current={item.match(path) ? "page" : undefined}>
					<Icon name={item.icon} />
					<span class="view-label">{item.label}</span>
				</a>
			{/each}
		</nav>

		<div class="actions">
			<PinToggle />
			<ThemeToggle />
			<a href="/help" class="icon-button" aria-label="Help" title="Help">
				<Icon name="help" />
			</a>
		</div>
	</header>

	<div class="content">
		{@render children()}
	</div>

	<Toasts />
	<PromptBar />

	<!-- The status footer: the main buffer's save state and caret, and the keys for this view. -->
	<footer class="status-bar">
		{#if path === "/"}
			<span class="save" data-save={$editorStatus.save} role="status">
				<span class="dot" aria-hidden="true"></span>{SAVE_LABEL[$editorStatus.save]}
			</span>
			{#if $editorStatus.line > 0}
				<span>Ln {$editorStatus.line}, Col {$editorStatus.col}</span>
			{/if}
		{/if}
		<span class="hints">
			{#each hints as [key, what] (key)}
				<span class="hint"><kbd>{key}</kbd>{what}</span>
			{/each}
		</span>
	</footer>
</div>

<style>
	.shell {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background: var(--color-base-100);
		color: var(--color-base-content);
	}

	/* The mockup's `h-12` bar with a 2px hairline at 20% ink. */
	.header {
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 0.75rem;
		height: 3rem;
		padding: 0 1rem;
		border-bottom: 2px solid color-mix(in srgb, var(--color-base-content) 20%, transparent);
		font-family: var(--font-mono);
		font-size: 0.75rem;
	}

	.brand {
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 0.5rem;
		color: inherit;
		text-decoration: none;
		--mark-size: 1.75rem;
	}

	.wordmark {
		font-family: var(--font-brand);
		font-size: 0.875rem;
		font-weight: 700;
		letter-spacing: -0.05em;
	}

	.search {
		display: flex;
		flex: 1;
		align-items: center;
		gap: 0.5rem;
		min-width: 0;
		max-width: 36rem;
		height: 2rem;
		margin: 0 auto;
		padding: 0 0.75rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		--icon-size: 1rem;
	}

	.search :global(.search-icon) {
		opacity: 0.6;
	}

	.search input {
		flex: 1;
		min-width: 0;
		border: 0;
		outline: 0;
		background: transparent;
		color: inherit;
		font: inherit;
	}

	.pill {
		flex: none;
		padding: 0.05rem 0.5rem;
		border-radius: 999px;
		background: var(--color-base-content);
		color: var(--color-base-100);
		font-size: 0.625rem;
		font-weight: 700;
		font-variant-numeric: tabular-nums;
	}

	.search kbd {
		padding: 0 0.4rem;
		border: 1px solid color-mix(in srgb, var(--color-base-content) 40%, transparent);
		border-radius: 999px;
		font: inherit;
		font-size: 0.65rem;
	}

	/* The join: three pills in one 2px outline; the current one is the inverse tile. */
	.views {
		display: flex;
		flex-shrink: 0;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		overflow: hidden;
		--icon-size: 0.875rem;
	}

	.view {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		padding: 0.2rem 0.65rem;
		color: inherit;
		font-weight: 700;
		text-decoration: none;
	}

	.view + .view {
		border-left: 2px solid var(--color-base-content);
	}

	.view[aria-current="page"] {
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.view:not([aria-current="page"]):hover {
		background: var(--color-base-200);
	}

	/* Icon-only below 1280px, as in the mockup (`xl:inline`). */
	@media (max-width: 1279px) {
		.view-label {
			display: none;
		}
	}

	.actions {
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 0.125rem;
		--icon-size: 1rem;
	}

	.actions :global(.icon-button) {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 1.75rem;
		height: 1.75rem;
		padding: 0;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: inherit;
		cursor: pointer;
	}

	.actions :global(.icon-button:hover),
	.actions :global(.icon-button[aria-pressed="true"]) {
		background: var(--color-base-200);
	}

	/* The mockup's footer row: h-8, 11px, 60% ink; the save state in full ink with its dot. */
	.status-bar {
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 1rem;
		height: 2rem;
		padding: 0 1rem;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
		font-family: var(--font-mono);
		font-size: 0.6875rem;
	}

	.save {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		min-width: 6rem;
		color: var(--color-base-content);
		font-weight: 700;
	}

	.dot {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 999px;
		background: var(--color-success);
	}

	.save[data-save="unsaved"] .dot,
	.save[data-save="saving"] .dot {
		background: var(--color-warning);
	}

	.save[data-save="refused"] .dot {
		background: var(--color-error);
	}

	.hints {
		display: flex;
		gap: 1rem;
		margin-left: auto;
	}

	.hint kbd {
		margin-right: 0.35rem;
		padding: 0 0.4rem;
		border: 1px solid color-mix(in srgb, var(--color-base-content) 40%, transparent);
		border-radius: 999px;
		font: inherit;
	}

	@media (max-width: 767px) {
		.hints {
			display: none;
		}
	}

	.content {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-height: 0;
		overflow: auto;
	}
</style>
