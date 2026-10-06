<script lang="ts">
	// Single-line edit popover (tasks/desktop-edit-popover, plan M7 / plan §3.2, design §2.3/§7):
	// one CM6 instance pre-filled with the raw line (hidden `id:` included — unlike the main view's
	// decoration, this popover shows everything), a token-chip row, inline strict-mode validation,
	// and a footer sourced from the daemon's `History`. Save/Cancel go through the Tauri command
	// bridge only (`$lib/daemon`) — never `fs`/raw sockets (design §7).
	//
	// This component performs NO `Apply` call itself — `onSave` is the single place the actual RPC
	// happens (see tasks/desktop-quick-add/notes.md's shared-`Popover` design: "the component knows
	// nothing about which window hosts it"). That is also a deliberate bugfix: an earlier revision
	// called `applyEdit` here AND relied on the host's `onSave` to call `applyMutations` again,
	// which double-applied every edit (two identical `edit_text` ops per save). Inverting control so
	// the host alone performs the write is what lets this same component serve both the main
	// view/detail view (`taskRef` set, host calls `applyEdit`) and quick-add (`taskRef` null — no
	// existing line yet, host calls `applyMutations(.., [{kind:"add", ...}])`) without either
	// duplicating writes or forking into two components.
	import { onMount } from "svelte";
	import { history as fetchHistory, type TaskRef } from "$lib/daemon";
	import LineEditor from "./LineEditor.svelte";
	import {
		CHIPS,
		decodeUlidTimestampMs,
		formatRelativeTime,
		isNoOpEdit,
		shortDevice
	} from "./editPopoverLogic";

	let {
		initialLine,
		taskRef,
		anchor,
		path,
		onSave,
		onCancel,
		onDirtyChange
	}: {
		initialLine: string;
		/** `null` for quick-add: there is no existing line yet, so no `Line N` footer and no
		 * `History` lookup (design: "no Line N footer — there is no line yet"). */
		taskRef: TaskRef | null;
		anchor: HTMLElement | null;
		path: string;
		/** Performs the actual write (the daemon's `Apply`) and resolves/rejects accordingly; this
		 * component only calls it, never `applyMutations`/`applyEdit` directly (see module doc). */
		onSave: (newLine: string) => void | Promise<void>;
		onCancel: () => void;
		/** Optional: fires on every keystroke with whether the text differs from `initialLine`.
		 * Wired by `MainView` to the quick-add hotkey's guard (tasks/desktop-quick-add/notes.md:
		 * "if the main popover is open and dirty, the hotkey focuses the main window instead") —
		 * unused by any other host, so it's optional rather than forcing one on every caller. */
		onDirtyChange?: (dirty: boolean) => void;
	} = $props();

	// The editor, chips and strict hint are `LineEditor` (task desktop-ui-revamp: one single-line
	// editor for the prompt bar, the Quick Add window and this popover).
	let editor: LineEditor | undefined = $state();
	// Only the "<device>, <relative time>" half comes from the async History lookup; "Line N" is
	// derived from the (fixed, for this popover's lifetime) `taskRef` prop.
	let historySuffix = $state("");
	// Empty (no footer text at all) when there's no existing line yet — quick-add's case.
	let footer = $derived.by(() => {
		if (!taskRef) return "";
		return historySuffix ? `Line ${taskRef.line_number} · ${historySuffix}` : `Line ${taskRef.line_number}`;
	});
	let saveError = $state("");

	/** Never blocked by a strict-mode hint (design §2.3): lenient save always wins. */
	async function saveAndClose(next: string) {
		if (isNoOpEdit(initialLine, next)) {
			onCancel();
			return;
		}
		saveError = "";
		try {
			await onSave(next);
		} catch (e) {
			saveError = String(e);
		}
	}

	async function loadFooter() {
		if (!taskRef?.task_id) return;
		try {
			const { ops } = await fetchHistory(path, taskRef.task_id, 1);
			const last = ops[0];
			if (!last) return;
			const when = formatRelativeTime(decodeUlidTimestampMs(last.op_id));
			historySuffix = `${shortDevice(last.device)}, ${when}`;
		} catch {
			// The footer's history lookup is a nicety, not load-bearing: keep the plain "Line N".
		}
	}

	function popoverStyle(): string {
		if (!anchor) return "";
		const rect = anchor.getBoundingClientRect();
		return `left: ${rect.left}px; top: ${rect.bottom + 4}px;`;
	}

	onMount(() => {
		loadFooter();
	});
</script>

<div class="popover" style={popoverStyle()} role="dialog" aria-label="Edit task">
	<div class="editor">
		<LineEditor
			bind:this={editor}
			initial={initialLine}
			autofocus
			onSubmit={saveAndClose}
			{onCancel}
			{onDirtyChange}
		/>
	</div>

	{#if saveError}
		<p class="error">{saveError}</p>
	{/if}

	<div class="chips">
		{#each CHIPS as c (c.chip)}
			<button type="button" onclick={() => editor?.chip(c.chip)}>{c.label}</button>
		{/each}
	</div>

	<div class="footer">
		<span class="location">{footer}</span>
		<div class="actions">
			<button type="button" onclick={onCancel}>Cancel</button>
			<button type="button" class="primary" onclick={() => saveAndClose(editor?.text() ?? initialLine)}>Save</button>
		</div>
	</div>
</div>

<style>
	.popover {
		position: fixed;
		z-index: 100;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		width: 26rem;
		max-width: calc(100vw - 2rem);
		padding: 0.75rem;
		border-radius: 8px;
		background: var(--popover-bg, var(--color-bg-elevated));
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
	}

	.editor {
		border: 1px solid var(--color-border);
		border-radius: 6px;
		padding: 0.25rem 0.5rem;
	}

	.error {
		margin: 0;
		font-size: 13px;
		color: var(--color-danger);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
	}

	.chips button {
		font-size: 12px;
		padding: 0.15rem 0.5rem;
		border-radius: 999px;
		border: 1px solid var(--color-border);
		background: var(--color-surface-muted);
		cursor: pointer;
	}

	.footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 12px;
		color: var(--color-text-muted);
	}

	.actions {
		display: flex;
		gap: 0.5rem;
	}

	.actions .primary {
		font-weight: 600;
	}
</style>
