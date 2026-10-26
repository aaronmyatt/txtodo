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
	import { editNotes, getNotes, readNoteAsset, saveNoteAsset, type NotesDoc, type TaskRef } from "$lib/daemon";
	import { imageFiles, imageMarkdown, noteImages } from "$lib/notes/noteImages";
	import { leaveEditor } from "$lib/escape";

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
	// The last load found no notes.md (the task had no `ref:` yet). The first save creates the
	// folder and the tag, so the parent must hear the new path, or it reads the tag plus the old
	// empty path as "an older daemon looked elsewhere" and swaps the editor for an error.
	let pathUnknown = false;
	// The notes.md path, workspace-relative: dropped images go in an `assets/` folder beside it.
	let notesPath = "";

	async function load() {
		try {
			const doc = await getNotes(task);
			loadError = "";
			lastSavedText = doc.text;
			pathUnknown = doc.path === "";
			notesPath = doc.path;
			view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: doc.text } });
			onLoaded?.(doc);
		} catch (e) {
			loadError = String(e);
		}
	}

	/** After the save that created notes.md: tell the parent where it is, without touching the
	 * editor (the reader may still be typing). */
	async function reportNewPath() {
		try {
			const doc = await getNotes(task);
			if (doc.path === "") return;
			pathUnknown = false;
			notesPath = doc.path;
			onLoaded?.(doc);
		} catch {
			// The save itself worked; the footer path just stays unknown until the next load.
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
			if (pathUnknown) await reportNewPath();
		} catch (e) {
			saveError = String(e);
		}
	}

	/** Saves each dropped or pasted image beside notes.md and links it at `at`. The first image of
	 * a task with no notes yet saves the notes first, which creates the folder (task
	 * enable-dragging-and-dropping-images-to-a). */
	async function addImages(files: File[], at: number) {
		if (!view || files.length === 0) return;
		saveError = "";
		try {
			if (!notesPath) {
				await editNotes(task, view.state.doc.toString());
				await reportNewPath();
				if (!notesPath) throw new Error("the notes folder could not be made");
			}
			const lines: string[] = [];
			for (const file of files) {
				const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
				lines.push(imageMarkdown(file.name, await saveNoteAsset(notesPath, file.name, bytes)));
			}
			const line = view.state.doc.lineAt(Math.min(at, view.state.doc.length));
			const insert = (line.length > 0 ? "\n" : "") + lines.join("\n") + "\n";
			view.dispatch({ changes: { from: line.to, insert } });
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
					noteImages({ notesPath: () => notesPath, read: readNoteAsset }),
					// Drop or paste an image file: it is saved and linked (`$lib/notes/noteImages.ts`).
					// https://codemirror.net/docs/ref/#view.EditorView^domEventHandlers
					EditorView.domEventHandlers({
						drop: (e, v) => {
							const files = imageFiles(e.dataTransfer?.files);
							if (files.length === 0) return false;
							e.preventDefault();
							const at = v.posAtCoords({ x: e.clientX, y: e.clientY }) ?? v.state.selection.main.head;
							void addImages(files, at);
							return true;
						},
						paste: (e, v) => {
							const files = imageFiles(e.clipboardData?.files);
							if (files.length === 0) return false;
							e.preventDefault();
							void addImages(files, v.state.selection.main.head);
							return true;
						}
					}),
					cmHistory(),
					// Esc saves what is pending at once and leaves the editor (`$lib/escape.ts`); notes
					// save as you type, so there is nothing to discard.
					keymap.of([
						{
							key: "Escape",
							run: (v) => {
								if (saveHandle) {
									clearTimeout(saveHandle);
									saveHandle = undefined;
									void save(v.state.doc.toString());
								}
								leaveEditor(v.contentDOM);
								return true;
							}
						},
						...historyKeymap,
						...defaultKeymap
					]),
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
	<!-- On a failed load the editor stays hidden (task desktop-notes-hidden): an empty editor
	     over a document that did not load invites typing, and the first keystroke would save an
	     empty-based text over whatever is really there. The shell must still exist for CM6. -->
	<div class="editor-shell" class:hidden={loadError !== ""} bind:this={containerEl}></div>
</div>

<style>
	.notes-editor {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.editor-shell.hidden {
		display: none;
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
