<script lang="ts">
	// Workspace offers from paired devices (specs/client-parity.toml offers.*; the TUI's `o`
	// pane), in Settings > Devices. Accept mirrors the workspace into the daemon's own folder;
	// decline drops the offer. Keys, with a row focused: j/k or the arrows move, a accepts, d
	// declines. `o` in list mode lands here (`/settings#offers`).
	import { onMount, tick } from "svelte";
	import { showToast } from "$lib/stores/toast";
	import { acceptOffer, declineOffer, workspaceOffers } from "./api";
	import { relativeTime } from "./time";
	import type { Offer } from "./types";

	let offers = $state<Offer[]>([]);
	let problem = $state("");
	let error = $state("");
	let busy = $state(false);
	let listEl: HTMLElement | undefined = $state();

	const key = (o: Offer) => `${o.offering_device}:${o.workspace_id}`;

	async function refresh() {
		try {
			const r = await workspaceOffers();
			offers = r.offers;
			problem = r.problem;
		} catch (e) {
			error = String(e);
		}
	}

	async function answer(o: Offer, accept: boolean) {
		if (busy) return;
		busy = true;
		error = "";
		try {
			if (accept) await acceptOffer(o.offering_device, o.workspace_id);
			else await declineOffer(o.offering_device, o.workspace_id);
			showToast(accept ? `Accepted ${o.name}: it is in your workspaces now` : `Declined ${o.name}`);
			await refresh();
			await tick();
			listEl?.querySelector<HTMLElement>("li button.row")?.focus();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	function onKeydown(e: KeyboardEvent, i: number) {
		const rows = [...(listEl?.querySelectorAll<HTMLElement>("li button.row") ?? [])];
		const go = (n: number) => rows[Math.max(0, Math.min(rows.length - 1, n))]?.focus();
		if (e.key === "j" || e.key === "ArrowDown") go(i + 1);
		else if (e.key === "k" || e.key === "ArrowUp") go(i - 1);
		else if (e.key === "a") void answer(offers[i], true);
		else if (e.key === "d") void answer(offers[i], false);
		else return;
		e.preventDefault();
	}

	onMount(() => {
		void refresh().then(async () => {
			// `o` brings the reader here: hand the keyboard to the first offer.
			if (location.hash !== "#offers") return;
			await tick();
			listEl?.querySelector<HTMLElement>("li button.row")?.focus();
		});
	});
</script>

<h3 id="offers">Workspace offers</h3>
{#if problem}
	<p class="problem" role="status">Offers from your devices are blocked: {problem}</p>
{/if}
<ul bind:this={listEl}>
	{#each offers as o, i (key(o))}
		<li>
			<button type="button" class="row" onkeydown={(e) => onKeydown(e, i)}>
				<span class="name">{o.name || o.workspace_id.slice(0, 8)}</span>
				<span class="from">from {o.offering_device.slice(0, 10)}… · {relativeTime(o.offered_at_ms)}</span>
			</button>
			<button type="button" class="pill solid" disabled={busy} onclick={() => answer(o, true)}>Accept</button>
			<button type="button" class="pill" disabled={busy} onclick={() => answer(o, false)}>Decline</button>
		</li>
	{:else}
		<li class="empty">{problem ? "Nothing can arrive until that is fixed." : "No offers waiting."}</li>
	{/each}
</ul>
{#if offers.length > 0}
	<p class="keys"><kbd>j</kbd> <kbd>k</kbd> move <kbd>a</kbd> accept <kbd>d</kbd> decline</p>
{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
	h3 {
		margin: 1.25rem 0 0.75rem;
		font-size: 0.875rem;
		scroll-margin-top: 1rem;
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
		padding: 0.5rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
	}

	li:first-child {
		border-top: 0;
	}

	.row {
		display: flex;
		flex: 1;
		flex-direction: column;
		align-items: flex-start;
		min-width: 0;
		padding: 0.2rem 0.4rem;
		border: 0;
		border-radius: var(--radius-field);
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: default;
	}

	.row:focus-visible {
		outline: 2px solid var(--color-base-content);
		outline-offset: 0;
	}

	.name {
		font-size: 0.875rem;
		font-weight: 700;
	}

	.from,
	.empty,
	.keys {
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-size: 0.75rem;
	}

	.keys {
		margin: 0.5rem 0 0;
	}

	.keys kbd {
		padding: 0 0.3rem;
		border: 1px solid currentColor;
		border-radius: 0.3rem;
		font: inherit;
	}

	.problem {
		color: var(--color-warning, #d97706);
		font-size: 0.8125rem;
	}

	.error {
		color: var(--color-error, crimson);
		font-size: 0.8125rem;
	}
</style>
