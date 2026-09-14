<script lang="ts">
	// Token pane (plan M6, design §6.2): create a scoped capability token, list existing ones with
	// their scopes (never the secret, except the one-time creation moment), revoke by id. All
	// caveat/macaroon logic lives in the daemon; this component only builds a `scopes: string[]`
	// from checkbox + restrictor state and calls `token_create`/`token_list`/`token_revoke`.
	import { onMount } from "svelte";
	import { tokenCreate, tokenList, tokenRevoke } from "./api";
	import { BASE_SCOPES, RESTRICTOR_PREFIXES, type RestrictorPrefix, type Scope, type Token } from "./types";

	let tokens = $state<Token[]>([]);
	let loadError = $state("");
	let loading = $state(true);

	let name = $state("");
	let checked = $state<Record<string, boolean>>({});
	let restrictors = $state<Record<RestrictorPrefix, string>>({ project: "", context: "", file: "" });
	let expiresInput = $state(""); // yyyy-mm-dd from <input type=date>, or empty = no expiry
	let createError = $state("");
	let creating = $state(false);

	/** Non-empty only immediately after a successful create — the one moment the secret exists. */
	let justCreated = $state<Token | null>(null);
	let copied = $state(false);

	let revokeError = $state("");
	let revokingId = $state("");

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

	function scopesFromForm(): Scope[] {
		const scopes: Scope[] = [];
		for (const s of BASE_SCOPES) {
			if (checked[s]) scopes.push(s);
		}
		for (const prefix of RESTRICTOR_PREFIXES) {
			const value = restrictors[prefix].trim();
			if (value) scopes.push(`${prefix}:${value}` as Scope);
		}
		return scopes;
	}

	function expiresRfc3339(): string {
		if (!expiresInput) return "";
		// `<input type=date>` gives "yyyy-mm-dd"; midnight UTC on that day, as RFC 3339.
		return new Date(`${expiresInput}T00:00:00Z`).toISOString();
	}

	async function createToken() {
		createError = "";
		const scopes = scopesFromForm();
		if (!name.trim()) {
			createError = "Name is required.";
			return;
		}
		if (scopes.length === 0) {
			createError = "Pick at least one scope.";
			return;
		}
		creating = true;
		try {
			const token = await tokenCreate(name.trim(), scopes, expiresRfc3339());
			justCreated = token;
			copied = false;
			name = "";
			checked = {};
			restrictors = { project: "", context: "", file: "" };
			expiresInput = "";
			await load();
		} catch (e) {
			createError = e instanceof Error ? e.message : String(e);
		} finally {
			creating = false;
		}
	}

	async function copySecret() {
		if (!justCreated?.secret) return;
		try {
			await navigator.clipboard.writeText(justCreated.secret);
			copied = true;
		} catch {
			// Clipboard permission denied or unavailable — the secret stays on screen to select
			// manually; nothing to fabricate here.
		}
	}

	function dismissSecret() {
		justCreated = null;
		copied = false;
	}

	async function revoke(id: string) {
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

<section class="tokens">
	<h2>Tokens</h2>

	<form onsubmit={(e) => { e.preventDefault(); createToken(); }}>
		<label>
			Name
			<input type="text" bind:value={name} placeholder="claude-code" />
		</label>

		<fieldset>
			<legend>Scopes</legend>
			{#each BASE_SCOPES as scope (scope)}
				<label class="checkbox">
					<input type="checkbox" bind:checked={checked[scope]} />
					{scope}
				</label>
			{/each}
		</fieldset>

		<fieldset>
			<legend>Restrict to (optional)</legend>
			<label>
				project:
				<input type="text" bind:value={restrictors.project} placeholder="+work" />
			</label>
			<label>
				context:
				<input type="text" bind:value={restrictors.context} placeholder="@phone" />
			</label>
			<label>
				file:
				<input type="text" bind:value={restrictors.file} placeholder="work.txt" />
			</label>
		</fieldset>

		<label>
			Expires
			<input type="date" bind:value={expiresInput} />
			<span class="hint">Empty = never expires.</span>
		</label>

		<button type="submit" disabled={creating}>{creating ? "Creating…" : "Create token"}</button>
		{#if createError}<p class="state-error">{createError}</p>{/if}
	</form>

	{#if justCreated}
		<div class="secret-reveal" role="alert">
			<p class="state-warning">Copy this now — it will not be shown again.</p>
			<code>{justCreated.secret}</code>
			<div class="actions">
				<button type="button" onclick={copySecret}>{copied ? "Copied" : "Copy"}</button>
				<button type="button" onclick={dismissSecret}>Done</button>
			</div>
		</div>
	{/if}

	<h3>Existing tokens</h3>
	{#if loading}
		<p>Loading…</p>
	{:else if loadError}
		<p class="state-error">{loadError}</p>
	{:else if tokens.length === 0}
		<p>No tokens yet.</p>
	{:else}
		{#if revokeError}<p class="state-error">{revokeError}</p>{/if}
		<ul>
			{#each tokens as t (t.id)}
				<li>
					<strong>{t.name}</strong>
					<span class="scopes">{t.scopes.join(", ")}</span>
					<span class="hint">{t.expires ? `expires ${t.expires}` : "no expiry"}</span>
					<button type="button" disabled={revokingId === t.id} onclick={() => revoke(t.id)}>
						{revokingId === t.id ? "Revoking…" : "Revoke"}
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<style>
	.tokens {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	form {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		max-width: 28rem;
	}
	fieldset {
		border: 1px solid var(--color-border);
		border-radius: 6px;
	}
	.checkbox {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		margin-right: 0.75rem;
	}
	.hint {
		color: var(--color-text-muted);
		font-size: 0.85rem;
	}
	.secret-reveal {
		border: 1px solid var(--color-warning-border);
		background: var(--color-warning-bg);
		border-radius: 8px;
		padding: 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		max-width: 28rem;
	}
	.secret-reveal code {
		word-break: break-all;
	}
	ul {
		list-style: none;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	li {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		border: 1px solid var(--color-border-subtle);
		border-radius: 6px;
		padding: 0.5rem 0.75rem;
	}
	.scopes {
		font-family: monospace;
		font-size: 0.85rem;
	}
	.state-error {
		color: var(--color-danger);
		font-weight: 600;
	}
	.state-warning {
		color: var(--color-warning-text);
		font-weight: 600;
	}
</style>
