<script lang="ts">
	// A read-only buffer that shows the editor prefs as they change (Settings > Appearance). Same
	// language, theme, gutter and long-line hint as the real FileView. pointer-events are off, so
	// the wheel scrolls the page, not the preview, and it never takes the keyboard.
	import { Compartment, EditorState } from "@codemirror/state";
	import { EditorView, highlightActiveLineGutter, lineNumbers } from "@codemirror/view";
	import { onMount } from "svelte";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { lineDecorations, longLineHint, mainViewBaseTheme } from "$lib/todotxt/decorations";
	import { editorPrefs } from "$lib/stores/editorPrefs";

	// Synthetic sample lines (never real data).
	const SAMPLE = [
		"(A) 2026-09-14 plan the Q4 roadmap +work @desk due:2026-09-26",
		"x 2026-09-18 2026-09-11 finish the onboarding doc +work",
		"2026-09-19 renew the passport +admin @online rec:1y",
		"(C) 2026-09-08 write down every idea for the side project so it stops rattling, then rank them by effort +ideas"
	].join("\n");

	let host: HTMLDivElement;
	let view: EditorView | undefined;
	const gutter = new Compartment();
	const hint = new Compartment();
	const gutterFor = (on: boolean) => (on ? [lineNumbers(), highlightActiveLineGutter()] : []);

	onMount(() => {
		view = new EditorView({
			state: EditorState.create({
				doc: SAMPLE,
				extensions: [
					todotxtLanguage,
					mainViewBaseTheme,
					lineDecorations("todo.txt", new Map()),
					gutter.of(gutterFor($editorPrefs.lineNumbers)),
					hint.of($editorPrefs.lengthHint ? longLineHint : []),
					// https://codemirror.net/docs/ref/#view.EditorView^editable
					// Wrapped, so the long sample line shows where the 100-character hint starts.
					EditorView.lineWrapping,
					EditorView.editable.of(false),
					EditorState.readOnly.of(true),
					EditorView.contentAttributes.of({ "aria-label": "Preview", tabindex: "-1" })
				]
			}),
			parent: host
		});
		return () => view?.destroy();
	});

	// Compartment.reconfigure: https://codemirror.net/docs/ref/#state.Compartment.reconfigure
	$effect(() => {
		const { lineNumbers: numbers, lengthHint } = $editorPrefs;
		view?.dispatch({ effects: [gutter.reconfigure(gutterFor(numbers)), hint.reconfigure(lengthHint ? longLineHint : [])] });
	});
</script>

<div class="preview" bind:this={host}></div>

<style>
	.preview {
		overflow: hidden;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 20%, transparent);
		border-radius: var(--radius-box);
		pointer-events: none;
	}
</style>
