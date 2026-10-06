<script lang="ts">
	// Settings > Appearance (task desktop-ui-revamp): the theme (the header toggle's store) and the
	// editor prefs (`editorPrefs.ts`), with a live preview buffer under them.
	import { setThemePreference, themePreference, type ThemePreference } from "$lib/stores/theme";
	import { editorPrefs, FONT_SIZES, setEditorPrefs } from "$lib/stores/editorPrefs";
	import EditorPreview from "./EditorPreview.svelte";
	import SettingsRow from "./SettingsRow.svelte";

	const THEMES: [ThemePreference, string][] = [
		["system", "System"],
		["light", "Light"],
		["dark", "Dark"]
	];
	const LINE_HEIGHTS: [number, string][] = [
		[24, "Tight"],
		[28, "Regular"],
		[32, "Roomy"]
	];
</script>

<SettingsRow label="Theme" description="System follows the OS.">
	<div class="join" role="group" aria-label="Theme">
		{#each THEMES as [value, label] (value)}
			<button type="button" class:on={$themePreference === value} aria-pressed={$themePreference === value} onclick={() => setThemePreference(value)}>
				{label}
			</button>
		{/each}
	</div>
</SettingsRow>
<SettingsRow label="Text size" for="s-size">
	<span class="range">
		<input
			id="s-size"
			type="range"
			class="d-range d-range-xs"
			min={FONT_SIZES.min}
			max={20}
			step="1"
			value={$editorPrefs.fontSize}
			oninput={(e) => setEditorPrefs({ fontSize: Number(e.currentTarget.value) })}
		/>
		<span class="value">{$editorPrefs.fontSize}px</span>
	</span>
</SettingsRow>
<SettingsRow label="Line height">
	<div class="join" role="group" aria-label="Line height">
		{#each LINE_HEIGHTS as [px, label] (px)}
			<button type="button" class:on={$editorPrefs.lineHeight === px} aria-pressed={$editorPrefs.lineHeight === px} onclick={() => setEditorPrefs({ lineHeight: px })}>
				{label}
			</button>
		{/each}
	</div>
</SettingsRow>
<SettingsRow label="Line numbers" for="s-numbers">
	<input id="s-numbers" type="checkbox" class="d-toggle" checked={$editorPrefs.lineNumbers} onchange={(e) => setEditorPrefs({ lineNumbers: e.currentTarget.checked })} />
</SettingsRow>
<SettingsRow label="Underline past 100 characters" description="A gentle nudge to keep lines short. Nothing stops a longer line." for="s-hint">
	<input id="s-hint" type="checkbox" class="d-toggle" checked={$editorPrefs.lengthHint} onchange={(e) => setEditorPrefs({ lengthHint: e.currentTarget.checked })} />
</SettingsRow>

<p class="caps">Preview</p>
<EditorPreview />

<style>
	.join {
		display: inline-flex;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		overflow: hidden;
	}

	.join button {
		padding: 0.15rem 0.7rem;
		border: 0;
		background: transparent;
		color: var(--color-base-content);
		font: inherit;
		font-size: 11px;
		font-weight: 700;
		cursor: pointer;
	}

	.join button + button {
		border-left: 2px solid var(--color-base-content);
	}

	.join button.on {
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.range {
		display: inline-flex;
		align-items: center;
		gap: 0.75rem;
	}

	.range input {
		width: 10rem;
	}

	.value {
		width: 2.5rem;
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		text-align: right;
	}

	.caps {
		margin: 1rem 0 0.5rem;
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}
</style>
