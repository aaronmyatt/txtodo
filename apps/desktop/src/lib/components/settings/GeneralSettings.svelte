<script lang="ts">
	// Settings > General (task desktop-ui-revamp): pin on top (the same store as the header pin),
	// what closing the window does today, save-on-blur (always on, decision 3), and the daemon's
	// status with Retry.
	import { onMount } from "svelte";
	import { daemonStatus, onDaemonStatus, retryConnect, type DaemonStatus } from "$lib/daemon";
	import { pinned, setPinned } from "$lib/stores/pin";
	import SettingsRow from "./SettingsRow.svelte";

	let status = $state<DaemonStatus>("connecting");
	let retrying = $state(false);

	async function retry() {
		retrying = true;
		try {
			status = await retryConnect();
		} finally {
			retrying = false;
		}
	}

	onMount(() => {
		void daemonStatus().then((s) => (status = s));
		const off = onDaemonStatus((s) => (status = s));
		return () => void off.then((f) => f());
	});
</script>

<SettingsRow label="Pin on top" description="Keep this window above other windows." for="s-pin">
	<input id="s-pin" type="checkbox" class="d-toggle" checked={$pinned} onchange={(e) => void setPinned(e.currentTarget.checked)} />
</SettingsRow>
<SettingsRow
	label="Keep running in the menu bar"
	description="Closing the window hides it; txtodo stays in the menu bar for the Quick Add hotkey. Quit from the menu bar icon."
>
	<span class="badge">always on</span>
</SettingsRow>
<SettingsRow
	label="Save when the buffer loses focus"
	description="Edits save as you go, and again whenever you leave the editor. Nothing waits for a Save button."
>
	<span class="badge">always on</span>
</SettingsRow>
<SettingsRow label="Daemon" description="txtodod keeps your files and syncs them. Retry reconnects, starting it if it is not running.">
	<span class="status" role="status"><span class="dot {status}"></span>{status}</span>
	<button type="button" class="retry" disabled={retrying || status === "connecting" || status === "spawning"} onclick={retry}>Retry</button>
</SettingsRow>

<style>
	.badge {
		padding: 0.1rem 0.6rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 30%, transparent);
		border-radius: 999px;
		font-size: 11px;
		font-weight: 700;
	}

	.status {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		margin-right: 0.75rem;
		font-size: 0.75rem;
		font-weight: 700;
	}

	.dot {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 999px;
		background: var(--color-warning, #d97706);
	}

	.dot.connected {
		background: var(--color-success, #15803d);
	}

	.dot.dead {
		background: var(--color-error, #dc2626);
	}

	.retry {
		padding: 0.2rem 0.8rem;
		border: 2px solid var(--color-base-content);
		border-radius: 999px;
		background: transparent;
		color: inherit;
		font: inherit;
		font-size: 0.75rem;
		font-weight: 700;
		cursor: pointer;
	}

	.retry:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
