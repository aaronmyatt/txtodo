<script lang="ts">
	// Paired devices (Settings > Devices, task desktop-ui-revamp): this device first, then the rest,
	// removed ones greyed. Remove asks first, inline: it rotates the group key, and the daemon's own
	// caveat (it does not un-share what the device already has) is shown after.
	import { onMount } from "svelte";
	import { showToast } from "$lib/stores/toast";
	import { deviceList, deviceRemove } from "./api";
	import { relativeTime } from "./time";
	import type { Device } from "./types";

	let devices = $state<Device[]>([]);
	let error = $state("");
	let confirming = $state<string | null>(null);
	let busy = $state(false);

	const ordered = $derived([...devices].sort((a, b) => Number(b.is_self) - Number(a.is_self) || Number(a.removed) - Number(b.removed)));

	async function refresh() {
		try {
			devices = await deviceList();
		} catch (e) {
			error = String(e);
		}
	}

	async function remove(d: Device) {
		confirming = null;
		busy = true;
		error = "";
		try {
			const result = await deviceRemove(d.id);
			if (result.message) showToast(result.message);
			await refresh();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	onMount(refresh);
</script>

<h3>Paired devices</h3>
<ul>
	{#each ordered as d (d.id)}
		<li class:removed={d.removed}>
			<div class="text">
				<span class="name">
					{d.name || d.id.slice(0, 8)}
					{#if d.is_self}<span class="badge on">this device</span>{/if}
					{#if d.own_device && !d.is_self}<span class="badge">yours</span>{/if}
					{#if d.removed}<span class="badge">removed</span>{/if}
				</span>
				<span class="seen">
					{#if d.is_self}now{:else if d.last_seen_ms > 0}seen {relativeTime(d.last_seen_ms)}{:else}not seen yet{/if}
				</span>
			</div>
			{#if confirming === d.id}
				<span class="confirm" role="alert">
					Rotates the key; it keeps what it already has.
					<button type="button" class="pill danger" disabled={busy} onclick={() => remove(d)}>Remove</button>
					<button type="button" class="pill" onclick={() => (confirming = null)}>Cancel</button>
				</span>
			{:else if !d.is_self && !d.removed}
				<button type="button" class="pill" disabled={busy} aria-label="Remove {d.name}" onclick={() => (confirming = d.id)}>Remove</button>
			{/if}
		</li>
	{:else}
		<li class="empty">No paired devices yet.</li>
	{/each}
</ul>
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
	h3 {
		margin: 0 0 0.75rem;
		font-size: 0.875rem;
	}

	ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 0.75rem;
		padding: 0.6rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	li:first-child {
		padding-top: 0;
		border-top: 0;
	}

	li.removed .text {
		opacity: 0.5;
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

	.seen,
	.empty {
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-size: 0.75rem;
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

	.confirm {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	.error {
		color: var(--color-error, crimson);
		font-size: 0.8125rem;
	}
</style>
