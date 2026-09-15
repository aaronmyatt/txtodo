<script lang="ts">
	// Workspace switcher (ADR 0025, task desktop-workspace-switcher): lists every registered
	// workspace, switches the active one, and adds/removes registry entries. Mounted in
	// MainView's top-nav; MainView wraps its file view in `{#key $currentWorkspaceRoot}` so a
	// switch remounts FileView/ConflictBanner — `switchWorkspace` only changes the selector
	// attached to calls made *after* it resolves, so anything already loaded (or an open `watch`
	// stream) has to be re-established against the new workspace, not just re-read in place.
	//
	// No folder-picker dialog: `@tauri-apps/plugin-dialog` isn't a dependency yet, so "add" takes a
	// typed absolute path, mirroring `txtodo workspace add <path>` on the CLI.
	import {
		addWorkspace,
		listWorkspaces,
		removeWorkspace,
		switchWorkspace,
		workspaceRoot,
		type WorkspaceInfo
	} from "$lib/daemon";
	import { currentWorkspaceRoot } from "$lib/stores/workspaces";

	let open = $state(false);
	let workspaces = $state<WorkspaceInfo[]>([]);
	let newPath = $state("");
	let error = $state("");
	let busy = $state(false);

	async function refresh() {
		workspaces = await listWorkspaces();
	}

	async function toggle() {
		open = !open;
		if (!open) return;
		error = "";
		try {
			await refresh();
		} catch (e) {
			error = String(e);
		}
	}

	async function pick(root: string) {
		if (root === $currentWorkspaceRoot || busy) return;
		error = "";
		busy = true;
		try {
			await switchWorkspace(root);
			$currentWorkspaceRoot = await workspaceRoot();
			open = false;
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function add(e: SubmitEvent) {
		e.preventDefault();
		const root = newPath.trim();
		if (!root || busy) return;
		error = "";
		busy = true;
		try {
			await addWorkspace(root);
			newPath = "";
			await refresh();
		} catch (e2) {
			error = String(e2);
		} finally {
			busy = false;
		}
	}

	async function remove(ws: WorkspaceInfo) {
		if (busy) return;
		error = "";
		busy = true;
		try {
			await removeWorkspace(ws.id, ws.root);
			await refresh();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}
</script>

<div class="switcher">
	<button type="button" class="trigger" onclick={toggle} aria-expanded={open}>
		{$currentWorkspaceRoot || "Workspace"}
	</button>
	{#if open}
		<div class="panel" role="menu">
			{#if error}
				<p class="error" role="alert">{error}</p>
			{/if}
			<ul>
				{#each workspaces as ws (ws.id)}
					<li class:current={ws.root === $currentWorkspaceRoot}>
						<button type="button" class="entry" onclick={() => pick(ws.root)} disabled={busy}>
							{ws.root}
						</button>
						{#if ws.root !== $currentWorkspaceRoot}
							<button
								type="button"
								class="remove"
								aria-label={`Remove ${ws.root}`}
								onclick={() => remove(ws)}
								disabled={busy}
							>
								&times;
							</button>
						{/if}
					</li>
				{/each}
			</ul>
			<form onsubmit={add}>
				<input type="text" placeholder="/path/to/workspace" bind:value={newPath} disabled={busy} />
				<button type="submit" disabled={busy}>Add</button>
			</form>
		</div>
	{/if}
</div>

<style>
	.switcher {
		position: relative;
	}

	.trigger {
		background: transparent;
		border: 1px solid var(--color-border, currentColor);
		border-radius: 6px;
		padding: 0.25rem 0.6rem;
		color: inherit;
		cursor: pointer;
		max-width: 22ch;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.panel {
		position: absolute;
		right: 0;
		top: calc(100% + 0.35rem);
		z-index: 10;
		min-width: 24rem;
		background: var(--color-bg);
		color: var(--color-text);
		border: 1px solid var(--color-border, currentColor);
		border-radius: 8px;
		padding: 0.5rem;
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
	}

	ul {
		list-style: none;
		margin: 0 0 0.5rem;
		padding: 0;
		max-height: 16rem;
		overflow-y: auto;
	}

	li {
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.entry {
		flex: 1;
		text-align: left;
		background: transparent;
		border: none;
		padding: 0.35rem 0.4rem;
		color: inherit;
		cursor: pointer;
		border-radius: 4px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	li.current .entry {
		font-weight: 600;
	}

	.entry:hover:not(:disabled) {
		background: var(--color-hover-bg, rgba(128, 128, 128, 0.15));
	}

	.remove {
		background: transparent;
		border: none;
		color: inherit;
		opacity: 0.6;
		cursor: pointer;
		padding: 0.2rem 0.45rem;
	}

	.remove:hover:not(:disabled) {
		opacity: 1;
	}

	form {
		display: flex;
		gap: 0.35rem;
	}

	form input {
		flex: 1;
		min-width: 0;
	}

	.error {
		margin: 0 0 0.5rem;
		color: var(--color-banner-text, crimson);
	}
</style>
