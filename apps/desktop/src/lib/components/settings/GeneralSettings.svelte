<script lang="ts">
	// Settings > General (task desktop-ui-revamp): pin on top (the same store as the header pin),
	// what closing the window does today, save-on-blur (always on, decision 3), the daemon's
	// status with Retry, and Updates (task self-upgrade): Check asks the txtodo CLI, Upgrade runs
	// it, Relaunch starts the new build. Nothing goes to the network until Check is pressed.
	import { onMount } from "svelte";
	import {
		checkUpdates,
		daemonStatus,
		onDaemonStatus,
		relaunch,
		retryConnect,
		runUpgrade,
		type DaemonStatus,
		type UpdateCheck
	} from "$lib/daemon";
	import { pinned, setPinned } from "$lib/stores/pin";
	import { changeNotices, setChangeNotices } from "$lib/stores/changeNotices";
	import SettingsRow from "./SettingsRow.svelte";

	let status = $state<DaemonStatus>("connecting");
	let retrying = $state(false);

	// The Updates row: what the last check said, and what is running now.
	let update = $state<UpdateCheck | null>(null);
	let updateNote = $state("");
	let updating = $state<"" | "checking" | "upgrading" | "done">("");

	async function check() {
		updating = "checking";
		updateNote = "";
		try {
			update = await checkUpdates();
			updateNote = update.newer ? `${update.latest} is out` : `${update.latest} is the newest`;
			updating = "";
		} catch (e) {
			updateNote = String(e);
			updating = "";
		}
	}

	async function upgrade() {
		updating = "upgrading";
		updateNote = update ? `Running: ${update.command}` : "";
		try {
			updateNote = await runUpgrade();
			updating = "done";
		} catch (e) {
			updateNote = String(e);
			updating = "";
		}
	}

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
	label="Notify on changes"
	description="A notification when an agent, another device or another app edits, completes or deletes tasks, while this window is in the background. Bursts arrive as one."
	for="s-notices"
>
	<input id="s-notices" type="checkbox" class="d-toggle" checked={$changeNotices} onchange={(e) => void setChangeNotices(e.currentTarget.checked)} />
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
<SettingsRow label="Updates" description="Checks GitHub for a newer release, then upgrades this device the way it was installed.">
	{#if updateNote}
		<span class="status" role="status">{updateNote}</span>
	{/if}
	{#if updating === "done"}
		<button type="button" class="retry" onclick={() => void relaunch()}>Relaunch</button>
	{:else if update?.newer}
		<button type="button" class="retry" disabled={updating !== ""} onclick={upgrade}>Upgrade</button>
	{:else}
		<button type="button" class="retry" disabled={updating !== ""} onclick={check}>Check</button>
	{/if}
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
