<script lang="ts">
	// The prompt bar (task desktop-ui-revamp, the c2 mockup's Quick Add on every screen): at the
	// foot of the main window, the shared single-line editor with the mark beside it and an Add
	// button. Enter adds the line to the open workspace's root list (the daemon stamps the date and
	// identity, never the UI) and keeps the keyboard for the next one; Esc hands the keyboard back to
	// the buffer. The nine chips float above the bar while it has the keyboard, so nothing shifts.
	// The strict-mode hint shows once typing pauses and never blocks (design §2.3).
	import { onMount } from "svelte";
	import { applyMutations, getFile, listFiles, onPromptBarFocus } from "$lib/daemon";
	import { addOnTopMutations } from "$lib/todotxt/addOnTop";
	import { editorStatus } from "$lib/stores/editorStatus";
	import Mark from "$lib/brand/Mark.svelte";
	import { meterFor, type Meter } from "$lib/brand/meter";
	import Icon from "$lib/icons/Icon.svelte";
	import { workspaceLayoutStore } from "$lib/stores/workspaces";
	import LineEditor from "./LineEditor.svelte";
	import { CHIPS } from "./editPopoverLogic";

	interface Props {
		/** The Quick Add window's own bar: chips always shown in the flow, no top rule, the field
		 * takes the keyboard on mount. */
		windowed?: boolean;
		/** Which list to add to, asked right before each add; the open workspace's root list by
		 * default. The Quick Add window has its own JS context, so it asks the daemon instead. */
		resolvePath?: () => Promise<string>;
		/** After a successful add (the window hides itself). */
		onAdded?: () => void;
		/** Esc; by default the keyboard goes back to the buffer. */
		onEscape?: () => void;
	}

	let { windowed = false, resolvePath, onAdded, onEscape }: Props = $props();

	let editor: LineEditor | undefined = $state();

	/** Gives the bar the keyboard: the `n` key (specs/client-parity.toml prompt.focus), resolved by
	 * AppShell. Svelte 5 component exports: https://svelte.dev/docs/svelte/bind#bind:this */
	export function focus() {
		editor?.focus();
	}
	let focused = $state(false);
	let error = $state("");
	let adding = $state(false);
	let hint = $state<string | null>(null);

	function currentPath(): Promise<string> {
		return resolvePath ? resolvePath() : Promise.resolve($workspaceLayoutStore.todo_file || "todo.txt");
	}

	// The live mark: the root list's done/total from the daemon. In the main window it is fetched
	// whenever the root buffer's own counts move (load, edits, remote changes, a workspace switch),
	// and only once the buffer has lines: a `ListFiles` in flight during startup cost the 10k-line
	// first paint ~500ms (e2e/perf.spec.ts). The Quick Add window has no buffer; it asks on mount,
	// which is every show. `meterSeq` drops a reply that lost to a newer refresh.
	let meter = $state<Meter | null>(null);
	let meterSeq = 0;
	async function refreshMeter() {
		const seq = ++meterSeq;
		try {
			const [path, files] = await Promise.all([currentPath(), listFiles()]);
			if (seq === meterSeq) meter = meterFor(files, path);
		} catch {
			// No daemon: the plain mark stays.
		}
	}

	$effect(() => {
		if (windowed) return;
		const { open, done, lines } = $editorStatus.counts;
		if (lines === 0) return;
		void [open, done];
		void refreshMeter();
	});

	async function add(line: string) {
		const text = line.trim();
		if (!text || adding) return;
		adding = true;
		error = "";
		try {
			// At the top of the list, like the TUI's add (2026-10-06 feedback): an `add` then a
			// `move_before` the first task, in one Apply, against the list as the daemon has it now.
			// A list the daemon does not have yet (a fresh workspace) just gets the plain add.
			const path = await currentPath();
			const doc = await getFile(path).catch(() => null);
			await applyMutations(path, doc ? addOnTopMutations(doc.text, doc.task_ids ?? [], text) : [{ kind: "add", line: text }]);
			editor?.setText("");
			onAdded?.();
		} catch (e) {
			error = String(e);
		} finally {
			adding = false;
		}
	}

	// The Quick Add hotkey with this window frontmost lands here (`quick_add.rs`).
	onMount(() => {
		if (windowed) {
			void refreshMeter();
			return;
		}
		const off = onPromptBarFocus(() => editor?.focus());
		return () => void off.then((f) => f());
	});

	/** Esc: back to the buffer (the root list's editor), or simply out of the bar. */
	function leave() {
		if (onEscape) return onEscape();
		const buffer = document.querySelector<HTMLElement>(".buffer .cm-content");
		if (buffer) buffer.focus();
		else (document.activeElement as HTMLElement | null)?.blur();
	}
</script>

<div class="prompt" class:focused class:windowed>
	{#if focused || windowed}
		<!-- mousedown is prevented so a chip never takes the keyboard from the editor. -->
		<div class="chips" role="toolbar" aria-label="Insert a token">
			{#each CHIPS as c (c.chip)}
				<button type="button" onmousedown={(e) => e.preventDefault()} onclick={() => editor?.chip(c.chip)}>
					{c.label}
				</button>
			{/each}
		</div>
	{/if}
	<div class="row">
		<Mark class="prompt-mark" progress={meter?.progress ?? null} title={meter?.title ?? "txtodo"} />
		<div class="field">
			<LineEditor
				bind:this={editor}
				placeholder="a new task +project @context"
				label="Add a task"
				autofocus={windowed}
				onSubmit={add}
				onCancel={leave}
				onFocusChange={(f) => (focused = f)}
				onHint={(h) => (hint = h)}
			/>
		</div>
		<button type="button" class="add" aria-label="Add task" disabled={adding} onclick={() => add(editor?.text() ?? "")}>
			Add <Icon name="enter" />
		</button>
	</div>
	<!-- One line kept free under the row, so the hint or an error never moves the bar. -->
	<p class="note" class:error={!!error} role="status">
		{#if error}Could not add: {error}{:else if hint}{hint}{/if}
	</p>
</div>

<style>
	.prompt {
		position: relative;
		flex-shrink: 0;
		padding: 0.75rem 1rem 0.25rem;
		border-top: 2px solid var(--color-base-content);
		background: var(--color-base-100);
	}

	/* The Quick Add window: the bar is the whole window, so no rule and the chips sit in the flow. */
	.windowed {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		height: 100%;
		box-sizing: border-box;
		padding: 0.75rem 1rem 0.25rem;
		border-top: 0;
	}

	.windowed .chips {
		position: static;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		--mark-size: 2.5rem;
	}

	.row :global(.prompt-mark) {
		flex: none;
	}

	/* The mockup's .tx-field: a 2px outline at 25% ink, full ink while it has the keyboard. */
	.field {
		flex: 1;
		min-width: 0;
		padding: 0.2rem 0.75rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 25%, transparent);
		border-radius: var(--radius-field);
		background: var(--color-base-100);
		font-size: 15px;
		transition: border-color 0.12s;
	}

	.focused .field {
		border-color: var(--color-base-content);
	}

	.add {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 0.4rem;
		padding: 0.55rem 1.1rem;
		border: 0;
		border-radius: 999px;
		background: var(--color-primary);
		color: var(--color-primary-content);
		font-family: var(--font-mono);
		font-size: 0.875rem;
		font-weight: 700;
		cursor: pointer;
		--icon-size: 1rem;
	}

	.add:disabled {
		opacity: 0.5;
	}

	/* Above the bar, out of the flow: focusing the bar shifts nothing. */
	.chips {
		position: absolute;
		bottom: 100%;
		left: 1rem;
		display: flex;
		flex-wrap: wrap;
		gap: 0.375rem;
		padding-bottom: 0.5rem;
	}

	.chips button {
		padding: 0.1rem 0.6rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		cursor: pointer;
	}

	.chips button:hover {
		background: var(--color-base-200);
	}

	.note {
		min-height: 1.25rem;
		margin: 0.25rem 0 0 3.25rem;
		color: var(--color-warning);
		font-size: 0.75rem;
		font-style: italic;
	}

	.note.error {
		color: var(--color-error);
		font-style: normal;
	}
</style>
