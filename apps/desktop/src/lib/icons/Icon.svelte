<script lang="ts">
	// One monoline icon from `icons.ts` (task desktop-ui-revamp). Sized by CSS (`--icon-size`,
	// default 1.25em, or any rule on `.icon`), never by width/height attributes, so a caller's style
	// always wins. Decorative by default; pass `label` when the icon is the only content of a control.
	// SVG strokes: https://developer.mozilla.org/en-US/docs/Web/SVG/Attribute/stroke-linecap
	import { ICONS, type IconName } from "./icons";

	interface Props {
		name: IconName;
		label?: string;
		class?: string;
	}

	let { name, label, class: className = "" }: Props = $props();
</script>

<svg
	class="icon {className}"
	viewBox="0 0 24 24"
	role={label ? "img" : undefined}
	aria-label={label}
	aria-hidden={label ? undefined : "true"}
>
	<!-- The markup is a constant from icons.ts, not user input. -->
	{@html ICONS[name]}
</svg>

<style>
	.icon {
		flex: none;
		width: var(--icon-size, 1.25em);
		height: var(--icon-size, 1.25em);
		fill: none;
		stroke: currentColor;
		stroke-width: 2.75;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
</style>
