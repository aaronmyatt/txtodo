<script lang="ts">
	// Cycles the app's theme preference (light -> dark -> system -> light), wired to
	// $lib/stores/theme.ts. One button rather than a 3-way picker keeps this at home in the
	// existing plain-button toolbar style (Breadcrumb's `.crumb`, EditPopover's `.chips`) instead
	// of introducing a new dropdown/menu pattern for a single, low-frequency action.
	import { cycleThemePreference, resolvedTheme, themePreference } from "$lib/stores/theme";

	const LABEL = { light: "Light", dark: "Dark", system: "Auto" } as const;
	const ICON = { light: "☀️", dark: "\u{1F319}", system: "\u{1F5A5}️" } as const;
</script>

<button
	type="button"
	class="theme-toggle"
	onclick={cycleThemePreference}
	title={`Theme: ${LABEL[$themePreference]} (currently showing ${$resolvedTheme}) — click to change`}
>
	<span aria-hidden="true">{ICON[$themePreference]}</span>
	{LABEL[$themePreference]}
</button>

<style>
	.theme-toggle {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.85rem;
		padding: 0.25rem 0.6rem;
		border-radius: 999px;
		border: 1px solid var(--color-border);
		background: var(--color-surface-muted);
		color: var(--color-text);
		cursor: pointer;
	}

	.theme-toggle:hover {
		background: var(--color-surface);
	}
</style>
