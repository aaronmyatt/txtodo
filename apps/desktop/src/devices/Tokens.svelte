<script lang="ts">
	// Token pane (plan M6, design §6.2): create a scoped capability token, list existing ones with
	// their scopes (never the secret, except the one-time creation moment), revoke by id. All
	// caveat/macaroon logic lives in the daemon; this component only builds a `scopes: string[]`
	// from checkbox + restrictor state and calls `token_create`/`token_list`/`token_revoke`.
	// Re-skinned for Settings > Tokens (task desktop-ui-revamp): a table, revoke behind a confirm,
	// and creation in NewTokenDialog.svelte.
	import { onMount } from "svelte";
	import { tokenList, tokenRevoke } from "./api";
	import type { Token } from "./types";
	import NewTokenDialog from "./NewTokenDialog.svelte";

	let newToken: NewTokenDialog | undefined = $state();

	let tokens = $state<Token[]>([]);
	let loadError = $state("");
	let loading = $state(true);

	let revokeError = $state("");
	let revokingId = $state("");
	let confirmingId = $state("");

	function dateOf(ms: number): string {
		return ms > 0 ? new Date(ms).toLocaleDateString() : "";
	}

	async function load() {
		loading = true;
		loadError = "";
		try {
			tokens = await tokenList();
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	onMount(load);

	async function revoke(id: string) {
		confirmingId = "";
		revokeError = "";
		revokingId = id;
		try {
			const ok = await tokenRevoke(id);
			if (!ok) {
				revokeError = "The daemon refused to revoke that token.";
				return;
			}
			await load();
		} catch (e) {
			revokeError = e instanceof Error ? e.message : String(e);
		} finally {
			revokingId = "";
		}
	}
</script>

<div class="head">
	<p class="lede">Let an agent or script use txtodo with only the rights you give it.</p>
	<button type="button" class="pill solid" onclick={() => newToken?.open()}>New token</button>
</div>

{#if loading}
	<p class="lede">Loading…</p>
{:else if loadError}
	<p class="state-error">{loadError}</p>
{:else if tokens.length === 0}
	<p class="lede">No tokens yet.</p>
{:else}
	{#if revokeError}<p class="state-error">{revokeError}</p>{/if}
	<table>
		<thead>
			<tr><th>Name</th><th>Scopes</th><th>Created</th><th>Expires</th><th><span class="sr">Actions</span></th></tr>
		</thead>
		<tbody>
			{#each tokens as t (t.id)}
				<tr>
					<td class="name">{t.name}</td>
					<td><span class="scopes">{#each t.scopes as sc (sc)}<span class="scope">{sc}</span>{/each}</span></td>
					<td class="dim">{dateOf(t.created_at_ms)}</td>
					<td class="dim">{t.expires ? t.expires.slice(0, 10) : "never"}</td>
					<td class="act">
						{#if confirmingId === t.id}
							<button type="button" class="pill danger" disabled={revokingId === t.id} onclick={() => revoke(t.id)}>
								{revokingId === t.id ? "Revoking…" : "Revoke"}
							</button>
							<button type="button" class="pill" onclick={() => (confirmingId = "")}>Cancel</button>
						{:else}
							<button type="button" class="pill" aria-label="Revoke {t.name}" onclick={() => (confirmingId = t.id)}>Revoke</button>
						{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
{/if}

<NewTokenDialog bind:this={newToken} onCreated={load} />

<style>
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
		margin-bottom: 1rem;
	}

	.lede,
	.dim {
		margin: 0;
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.8125rem;
	}

	th {
		padding: 0 0.5rem 0.4rem 0;
		color: color-mix(in srgb, var(--color-base-content) 60%, transparent);
		font-size: 11px;
		letter-spacing: 0.05em;
		text-align: left;
		text-transform: uppercase;
	}

	td {
		padding: 0.5rem 0.5rem 0.5rem 0;
		border-top: 2px solid color-mix(in srgb, var(--color-base-content) 10%, transparent);
		vertical-align: middle;
	}

	.name {
		font-weight: 700;
	}

	.scopes {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
	}

	.scope {
		padding: 0 0.4rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 25%, transparent);
		border-radius: 999px;
		font-size: 11px;
	}

	.act {
		white-space: nowrap;
		text-align: right;
	}

	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
	}

	.pill {
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

	.state-error {
		margin: 0;
		color: var(--color-error, #dc2626);
		font-weight: 700;
		font-size: 0.8125rem;
	}

</style>
