<script lang="ts">
	// Proof-of-pipeline main page (the real main view is a separate task): calls `list_files` on
	// mount over the Tauri command bridge and renders what comes back, and shows a reconnect
	// banner driven by `daemon-status` events/`daemon_status` so a missing/dead daemon never
	// looks like a crash (design §5).
	import { onMount } from "svelte";
	import {
		daemonStatus,
		listFiles,
		onDaemonStatus,
		retryConnect,
		type DaemonStatus,
		type FileInfo
	} from "$lib/daemon";

	let files = $state<FileInfo[]>([]);
	let status = $state<DaemonStatus>("connecting");
	let error = $state("");
	let loading = $state(true);

	async function load() {
		loading = true;
		error = "";
		try {
			files = await listFiles();
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	}

	async function retry() {
		status = await retryConnect();
		if (status === "connected") await load();
	}

	onMount(() => {
		daemonStatus().then((s) => (status = s));
		const unlisten = onDaemonStatus((s) => {
			status = s;
			if (s === "connected") load();
		});
		load();
		return () => {
			unlisten.then((f) => f());
		};
	});
</script>

<main class="container">
	{#if status !== "connected"}
		<div class="banner" role="alert">
			<span>Daemon: {status}{error ? ` — ${error}` : ""}</span>
			<button onclick={retry}>Retry</button>
		</div>
	{/if}

	<h1>txtodo</h1>

	{#if loading}
		<p>Loading files…</p>
	{:else if files.length === 0}
		<p>No files yet.</p>
	{:else}
		<ul>
			{#each files as f (f.path)}
				<li><code>{f.path}</code> — {f.kind} ({f.done}/{f.total})</li>
			{/each}
		</ul>
	{/if}
</main>

<style>
	.container {
		padding: 2rem;
		font-family:
			Inter,
			Avenir,
			Helvetica,
			Arial,
			sans-serif;
	}

	.banner {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		background: #fde68a;
		color: #1f2937;
		padding: 0.5rem 1rem;
		border-radius: 6px;
		margin-bottom: 1rem;
	}

	ul {
		padding-left: 1.25rem;
	}
</style>
