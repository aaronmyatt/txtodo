<script lang="ts">
	// notes.md editor (tasks/desktop-detail-view, plan §3.2): plain CM6 markdown editing, no
	// WYSIWYG. Debounced whole-document `editNotes` calls — the daemon derives the Loro text ops
	// itself from the new full text (dto_notes.rs: "the daemon derives the Loro text ops"), so this
	// component never diffs anything; it just ships the current doc after a short pause in typing.
	//
	// Lazy creation (plan §3.2.4): the FIRST successful `editNotes` call for a task with no `ref:`
	// tag yet is what mints the tag + directory server-side
	// (`crates/txtodo-daemon/src/refdir_ops.rs::ensure_ref_dir`) — this component doesn't know or
	// care whether that's the case; it always just calls `editNotes(task, text)`.
	//
	// CM6 markdown language: https://codemirror.net/docs/ref/#lang-markdown
	import { onDestroy, onMount } from "svelte";
	import { EditorState } from "@codemirror/state";
	import { EditorView, keymap } from "@codemirror/view";
	import { defaultKeymap, history as cmHistory, historyKeymap } from "@codemirror/commands";
	import { markdown } from "@codemirror/lang-markdown";
	import { editNotes, getNotes, type NotesDoc, type TaskRef } from "$lib/daemon";

	const SAVE_DEBOUNCE_MS = 500;

	// `onLoaded` hands the daemon's answer to the parent, which shows `path` in its footer and opens a
	// collapsed notes section when there is text (task desktop-notes-hidden).
	let { task, onLoaded }: { task: TaskRef; onLoaded?: (doc: NotesDoc) => void } = $props();

	let containerEl: HTMLDivElement | undefined;
	let view: EditorView | undefined;
	let loadError = $state("");
	let saveError = $state("");
	let saveHandle: ReturnType<typeof setTimeout> | undefined;
	let lastSavedText = ""; // guards the no-op case: focus-out with nothing typed saves nothing.

	async function load() {
		try {
			const doc = await getNotes(task);
			loadError = "";
			lastSavedText = doc.text;
			view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: doc.text } });
			onLoaded?.(doc);
		} catch (e) {
			loadError = String(e);
		}
	}

	function scheduleSave(text: string) {
		if (saveHandle) clearTimeout(saveHandle);
		saveHandle = setTimeout(() => save(text), SAVE_DEBOUNCE_MS);
	}

	async function save(text: string) {
		if (text === lastSavedText) return; // identical content: no Apply, matching the popover's rule
		saveError = "";
		try {
			await editNotes(task, text);
			lastSavedText = text;
		} catch (e) {
			saveError = String(e);
		}
	}

	onMount(() => {
		view = new EditorView({
			state: EditorState.create({
				doc: "",
				extensions: [
					EditorView.lineWrapping,
					markdown(),
					cmHistory(),
					keymap.of([...historyKeymap, ...defaultKeymap]),
					EditorView.updateListener.of((u) => {
						if (u.docChanged) scheduleSave(u.state.doc.toString());
					})
				]
			}),
			parent: containerEl
		});
		load();
	});

	onDestroy(() => {
		if (saveHandle) {
			clearTimeout(saveHandle);
			// Flush a pending debounced save rather than dropping the human's last keystrokes.
			if (view) save(view.state.doc.toString());
		}
		view?.destroy();
	});

	// A future re-open of the same DetailView instance on a different task (parent renamed the
	// slug, or the human navigated) should re-baseline rather than keep showing stale notes.
	// svelte-ignore state_referenced_locally -- intentional: tracks task.task_id's *previous*
	// value to detect a change, not a live derivation of it (same pattern as FileView.svelte).
	let loadedFor = task.task_id;
	$effect(() => {
		if (task.task_id !== loadedFor && view) {
			loadedFor = task.task_id;
			load();
		}
	});
</script>

<div class="notes-editor">
	{#if loadError}
		<p class="error" role="alert">Could not load notes: {loadError}</p>
	{/if}
	{#if saveError}
		<p class="error" role="alert">Could not save notes: {saveError}</p>
	{/if}
	<div class="editor-shell" bind:this={containerEl}></div>
</div>

<style>
	.notes-editor {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.editor-shell {
		border: 1px solid var(--color-border-subtle);
		border-radius: 6px;
		min-height: 6rem;
		max-height: 40vh;
		overflow: auto;
	}

	.error {
		margin: 0;
		font-size: 0.85rem;
		color: var(--color-danger);
	}
</style>
