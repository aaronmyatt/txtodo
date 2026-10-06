<script lang="ts">
	// Settings > Workspaces (task desktop-ui-revamp): every registered workspace with open and
	// remove, plus add by path (the same calls as the header switcher). Remove asks first, inline,
	// and says the files stay on disk: it only unregisters the folder.
	import { goto } from "$app/navigation";
	import { onMount } from "svelte";
	import { addWorkspace, listWorkspaces, removeWorkspace, switchWorkspace, workspaceRoot, type WorkspaceInfo } from "$lib/daemon";
	import { currentWorkspaceRoot } from "$lib/stores/workspaces";
	import { workspaceLabel } from "$lib/universal/rows";
	// The native folder picker: https://v2.tauri.app/plugin/dialog/#open-a-file-selector-dialog
	import { open as openDialog } from "@tauri-apps/plugin-dialog";

	// Only a real Tauri webview has the dialog plugin; `npm run dev` and Playwright keep the path
	// field alone.
	const canPick = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

	let list = $state<WorkspaceInfo[]>([]);
	let error = $state("");
	let busy = $state(false);
	let confirming = $state<string | null>(null);
	let newPath = $state("");

	async function refresh() {
		try {
			list = await listWorkspaces();
		} catch (e) {
			error = String(e);
		}
	}

	/** Runs one change with the busy guard and error line, then reloads the list. */
	async function run(change: () => Promise<unknown>) {
		if (busy) return;
		error = "";
		busy = true;
		try {
			await change();
			await refresh();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	function open(ws: WorkspaceInfo) {
		void run(async () => {
			if (ws.root !== $currentWorkspaceRoot) {
				await switchWorkspace(ws.root);
				$currentWorkspaceRoot = await workspaceRoot();
			}
			await goto("/");
		});
	}

	function remove(ws: WorkspaceInfo) {
		confirming = null;
		void run(() => removeWorkspace(ws.id, ws.root));
	}

	/** Choose a folder with the OS picker and add it straight away. */
	function pick() {
		void run(async () => {
			const root = await openDialog({ directory: true, multiple: false, title: "Add a workspace folder" });
			if (typeof root === "string" && root) await addWorkspace(root);
		});
	}

	function add(e: SubmitEvent) {
		e.preventDefault();
		const root = newPath.trim();
		if (!root) return;
		void run(async () => {
			await addWorkspace(root);
			newPath = "";
		});
	}

	onMount(() => {
		void refresh();
		// Settings can be the first page shown, before the main view has asked which is open.
		if (!$currentWorkspaceRoot) void workspaceRoot().then((r) => ($currentWorkspaceRoot = r));
	});
</script>

<ul class="list">
	{#each list as ws (ws.id)}
		<li class:missing={!ws.root_exists}>
			<span class="tile" aria-hidden="true">{(ws.name || workspaceLabel(ws.root)).slice(0, 2)}</span>
			<div class="text">
				<span class="name">
					{ws.name || workspaceLabel(ws.root)}
					{#if ws.root === $currentWorkspaceRoot}<span class="badge on">open</span>{/if}
					{#if ws.is_default}<span class="badge">default</span>{/if}
					{#if ws.is_remote}<span class="badge">remote</span>{/if}
					{#if !ws.root_exists}<span class="badge warn" title="This folder no longer exists on disk">missing</span>{/if}
				</span>
				<span class="path">{ws.root}</span>
			</div>
			{#if confirming === ws.id}
				<span class="confirm" role="alert">
					Files stay on disk.
					<button type="button" class="pill danger" disabled={busy} onclick={() => remove(ws)}>Remove</button>
					<button type="button" class="pill" onclick={() => (confirming = null)}>Cancel</button>
				</span>
			{:else}
				<button type="button" class="pill" disabled={busy || !ws.root_exists} onclick={() => open(ws)}>Open</button>
				{#if !ws.is_default}
					<button type="button" class="pill" disabled={busy} aria-label="Remove {ws.name || workspaceLabel(ws.root)}" onclick={() => (confirming = ws.id)}>Remove</button>
				{/if}
			{/if}
		</li>
	{/each}
</ul>

<form class="add" onsubmit={add}>
	{#if canPick}
		<button type="button" class="pill solid" disabled={busy} onclick={pick}>Choose a folder…</button>
		<span class="or">or type a path</span>
	{/if}
	<label for="s-ws-path" class:sr={canPick}>Add a folder</label>
	<input id="s-ws-path" type="text" placeholder="/path/to/folder" bind:value={newPath} spellcheck="false" autocomplete="off" />
	<button type="submit" class="pill solid" disabled={busy || newPath.trim() === ""}>Add</button>
</form>
<p class="hint">Workspaces you use from the CLI or TUI show up here on their own.</p>
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
	.list {
		display: flex;
		flex-direction: column;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	li {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.6rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	li:first-child {
		padding-top: 0;
		border-top: 0;
	}

	li.missing .text {
		opacity: 0.55;
	}

	.tile {
		display: grid;
		flex-shrink: 0;
		place-items: center;
		width: 1.75rem;
		height: 1.75rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 40%, transparent);
		border-radius: 0.5rem;
		font-size: 11px;
		font-weight: 700;
	}

	.text {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-width: 0;
	}

	.name {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.875rem;
		font-weight: 700;
	}

	.path {
		overflow: hidden;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-size: 0.75rem;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.badge {
		padding: 0 0.45rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 30%, transparent);
		border-radius: 999px;
		font-size: 10px;
		font-weight: 700;
	}

	.badge.on {
		border-color: var(--color-base-content);
		background: var(--color-base-content);
		color: var(--color-base-100);
	}

	.badge.warn {
		border-color: var(--color-warning, #d97706);
		color: var(--color-warning, #d97706);
	}

	.confirm {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	.pill {
		flex-shrink: 0;
		padding: 0.2rem 0.8rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		cursor: pointer;
	}

	.pill:disabled {
		opacity: 0.45;
		cursor: default;
	}

	.pill.solid {
		border-color: var(--color-primary);
		background: var(--color-primary);
		color: var(--color-primary-content);
	}

	.pill.danger {
		border-color: var(--color-error, #dc2626);
		background: var(--color-error, #dc2626);
		color: var(--color-error-content, #fff);
	}

	.add {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 0.75rem;
		margin-top: 1rem;
	}

	.add label {
		font-size: 0.875rem;
		font-weight: 700;
	}

	.add input {
		flex: 1 1 14rem;
		padding: 0.3rem 0.75rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 25%, transparent);
		border-radius: var(--radius-field);
		background: var(--color-base-100);
		color: inherit;
		font: inherit;
		font-size: 0.8125rem;
	}

	.add input:focus {
		border-color: var(--color-base-content);
		outline: none;
	}

	.or {
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	/* Visually hidden, still the field's name. */
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
	}

	.hint {
		margin: 0.5rem 0 0;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	.error {
		margin: 0.5rem 0 0;
		color: var(--color-error, crimson);
		font-size: 0.8125rem;
	}
</style>
