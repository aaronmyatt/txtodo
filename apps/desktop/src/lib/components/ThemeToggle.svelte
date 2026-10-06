<script lang="ts">
	// Cycles the app's theme preference (light -> dark -> system -> light), wired to
	// $lib/stores/theme.ts. An icon button in the header (task desktop-ui-revamp): the sun, the
	// moon, or the laptop for "follow the system"; the title says which and what is showing.
	import { cycleThemePreference, resolvedTheme, themePreference } from "$lib/stores/theme";
	import Icon from "$lib/icons/Icon.svelte";

	const LABEL = { light: "Light", dark: "Dark", system: "Auto" } as const;
	const ICON = { light: "sun", dark: "moon", system: "laptop" } as const;
</script>

<button
	type="button"
	class="icon-button theme-toggle"
	onclick={cycleThemePreference}
	aria-label={`Theme: ${LABEL[$themePreference]}`}
	title={`Theme: ${LABEL[$themePreference]} (currently showing ${$resolvedTheme}) — click to change`}
>
	<Icon name={ICON[$themePreference]} />
</button>
