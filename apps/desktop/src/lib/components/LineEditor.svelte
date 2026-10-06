<script lang="ts">
	// The single-line todo.txt editor every one-line entry shares (task desktop-ui-revamp): the
	// prompt bar, the Quick Add window and the edit popover. One CM6 instance with the todo.txt
	// highlighting and undo, kept to one line, Enter submits and Esc cancels; the nine chips, and
	// the shared core's strict-mode hint once typing pauses (it never blocks: design §2.3, lenient
	// in). The host owns what a submit does (`onSubmit`); this only edits text.
	// Ref: https://codemirror.net/docs/ref/ (EditorState, EditorView, keymap, Prec)
	import { onDestroy, onMount } from "svelte";
	import { EditorState, Prec } from "@codemirror/state";
	import { EditorView, keymap, placeholder as cmPlaceholder } from "@codemirror/view";
	import { defaultKeymap, history as cmHistory, historyKeymap } from "@codemirror/commands";
	import { todotxtLanguage } from "$lib/lang/todotxtLanguage";
	import { singleLineFilter } from "$lib/todotxt/singleLineFilter";
	import { strictHint } from "$lib/wasmCore";
	import { applyChip, localToday, type Chip } from "./editPopoverLogic";

	interface Props {
		/** The text it starts with. */
		initial?: string;
		placeholder?: string;
		/** Takes the keyboard when it mounts. */
		autofocus?: boolean;
		/** Enter: the line as typed. The host decides what that means (add, edit) and may throw. */
		onSubmit: (line: string) => void | Promise<void>;
		/** Esc. */
		onCancel: () => void;
		/** Every change: whether the text differs from `initial`. */
		onDirtyChange?: (dirty: boolean) => void;
		/** The editor gained or lost the keyboard (the prompt bar floats its chips while it has it). */
		onFocusChange?: (focused: boolean) => void;
		/** Accessible name of the field. */
		label?: string;
		/** Takes the strict hint to show it elsewhere (the prompt bar puts it under its row);
		 * without it the hint shows under the field. */
		onHint?: (hint: string | null) => void;
	}

	let {
		initial = "",
		placeholder = "",
		autofocus = false,
		onSubmit,
		onCancel,
		onDirtyChange,
		onFocusChange,
		label = "Task",
		onHint
	}: Props = $props();

	const HINT_AFTER_MS = 400;

	let host: HTMLDivElement | undefined;
	let view: EditorView | undefined;
	let hint = $state<string | null>(null);
	let hintTimer: ReturnType<typeof setTimeout> | undefined;

	/** The text right now. */
	export function text(): string {
		return view ? view.state.doc.toString() : initial;
	}

	/** Replaces the text (and puts the caret at its end). */
	export function setText(next: string) {
		if (!view) return;
		view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next }, selection: { anchor: next.length } });
	}

	/** Gives the editor the keyboard. */
	export function focus() {
		view?.focus();
	}

	/** One chip tap (`editPopoverLogic.applyChip`, the core's chip rules), keeping the keyboard. */
	export function chip(c: Chip) {
		if (!view) return;
		const raw = text();
		const caret = view.state.selection.main.head;
		const { text: next, caret: at } = applyChip(raw, caret, c, localToday());
		view.dispatch({ changes: { from: 0, to: raw.length, insert: next }, selection: { anchor: at } });
		view.focus();
	}

	function scheduleHint(line: string) {
		clearTimeout(hintTimer);
		hint = null;
		onHint?.(null);
		hintTimer = setTimeout(async () => {
			hint = line.trim() === "" ? null : await strictHint(line);
			onHint?.(hint);
		}, HINT_AFTER_MS);
	}

	onMount(() => {
		view = new EditorView({
			state: EditorState.create({
				doc: initial,
				extensions: [
					todotxtLanguage,
					cmHistory(),
					singleLineFilter(),
					cmPlaceholder(placeholder),
					EditorView.contentAttributes.of({ "aria-label": label }),
					// Enter submits, Esc cancels, ahead of the default keymap's newline.
					// https://codemirror.net/docs/ref/#state.Prec
					Prec.highest(
						keymap.of([
							{
								key: "Enter",
								preventDefault: true,
								run: () => {
									void onSubmit(text());
									return true;
								}
							},
							{
								key: "Escape",
								preventDefault: true,
								run: () => {
									onCancel();
									return true;
								}
							}
						])
					),
					keymap.of([...historyKeymap, ...defaultKeymap]),
					EditorView.updateListener.of((u) => {
						if (u.focusChanged) onFocusChange?.(u.view.hasFocus);
						if (!u.docChanged) return;
						const line = u.state.doc.toString();
						scheduleHint(line);
						onDirtyChange?.(line !== initial);
					})
				]
			}),
			parent: host
		});
		if (autofocus) view.focus();
		if (initial) scheduleHint(initial);
	});

	onDestroy(() => {
		clearTimeout(hintTimer);
		view?.destroy();
		onDirtyChange?.(false);
	});
</script>

<div class="line-editor">
	<div class="field" bind:this={host}></div>
	{#if hint && !onHint}
		<p class="hint" role="status">{hint}</p>
	{/if}
</div>

<style>
	.line-editor {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		min-width: 0;
	}

	.field {
		min-width: 0;
	}

	.field :global(.cm-editor) {
		outline: none;
	}

	.hint {
		margin: 0;
		font-size: 0.75rem;
		font-style: italic;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
	}
</style>
