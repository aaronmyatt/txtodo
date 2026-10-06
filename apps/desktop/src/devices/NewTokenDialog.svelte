<script lang="ts">
	// The New token dialog (Settings > Tokens, task desktop-ui-revamp): name, the closed scope set
	// with plain words (design §6.2), project/context/file restrictors and an expiry. On create the
	// secret is shown once, in memory only, with Copy; it is never logged or stored, and Done or
	// closing the dialog drops it.
	import { tokenCreate } from "./api";
	import { BASE_SCOPES, RESTRICTOR_PREFIXES, type RestrictorPrefix, type Scope, type Token } from "./types";

	let { onCreated }: { onCreated: () => void | Promise<void> } = $props();

	let name = $state("");
	let checked = $state<Record<string, boolean>>({});
	let restrictors = $state<Record<RestrictorPrefix, string>>({ project: "", context: "", file: "" });
	let expiresInput = $state(""); // yyyy-mm-dd from <input type=date>, or empty = no expiry
	let createError = $state("");
	let creating = $state(false);

	/** Non-empty only immediately after a successful create — the one moment the secret exists. */
	let justCreated = $state<Token | null>(null);
	let copied = $state(false);

	/** The New token dialog: a native modal, so focus is trapped and Esc closes it.
	 * https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/dialog */
	let dialog: HTMLDialogElement | undefined = $state();

	/** Each base scope in plain words (design §6.2's closed set). */
	const SCOPE_WORDS: Record<string, string> = {
		read: "See every task",
		"write:add": "Add tasks",
		"write:complete": "Complete tasks",
		"write:edit": "Edit task text",
		"write:delete": "Delete tasks",
		raw: "Raw file access. Skips every restriction below."
	};

	/** Opens the dialog, fresh. */
	export function open() {
		createError = "";
		dialog?.showModal();
	}

	function closeDialog() {
		dialog?.close();
		dismissSecret();
	}

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
			// The secret lives only here, in memory, until Done; it is never logged or stored.
			justCreated = token;
			copied = false;
			name = "";
			checked = {};
			restrictors = { project: "", context: "", file: "" };
			expiresInput = "";
			await onCreated();
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
</script>

<dialog bind:this={dialog} aria-label="New token" onclose={dismissSecret}>
	{#if justCreated}
		<div class="secret-reveal" role="alert">
			<h3>Token created</h3>
			<p class="state-warning">Copy this now. It will not be shown again.</p>
			<code>{justCreated.secret}</code>
			<div class="actions">
				<button type="button" class="pill solid" onclick={copySecret}>{copied ? "Copied" : "Copy"}</button>
				<button type="button" class="pill" onclick={closeDialog}>Done</button>
			</div>
		</div>
	{:else}
		<form onsubmit={(e) => { e.preventDefault(); createToken(); }}>
			<h3>New token</h3>
			<label class="field">
				<span>Name</span>
				<input type="text" bind:value={name} placeholder="claude-code" />
			</label>

			<fieldset>
				<legend>Scopes</legend>
				{#each BASE_SCOPES as scope (scope)}
					<label class="checkbox">
						<input type="checkbox" bind:checked={checked[scope]} />
						<code>{scope}</code>
						<span class="dim">{SCOPE_WORDS[scope] ?? ""}</span>
					</label>
				{/each}
			</fieldset>

			<fieldset>
				<legend>Restrict to (optional)</legend>
				<label class="field"><span>project:</span><input type="text" bind:value={restrictors.project} placeholder="+work" /></label>
				<label class="field"><span>context:</span><input type="text" bind:value={restrictors.context} placeholder="@phone" /></label>
				<label class="field"><span>file:</span><input type="text" bind:value={restrictors.file} placeholder="work.txt" /></label>
			</fieldset>

			<label class="field">
				<span>Expires</span>
				<input type="date" bind:value={expiresInput} />
				<span class="dim">Empty = never.</span>
			</label>

			{#if createError}<p class="state-error">{createError}</p>{/if}
			<div class="actions">
				<button type="submit" class="pill solid" disabled={creating}>{creating ? "Creating…" : "Create token"}</button>
				<button type="button" class="pill" onclick={closeDialog}>Cancel</button>
			</div>
		</form>
	{/if}
</dialog>

<style>
	dialog {
		width: min(32rem, calc(100vw - 2rem));
		padding: 1.5rem;
		border: 2px solid var(--color-base-content);
		border-radius: var(--radius-box);
		background: var(--color-base-100);
		color: var(--color-base-content);
		font-family: var(--font-mono);
	}

	dialog::backdrop {
		background: color-mix(in srgb, #000 40%, transparent);
	}

	form,
	.secret-reveal {
		display: flex;
		flex-direction: column;
		gap: 0.9rem;
	}

	h3 {
		margin: 0;
		font-size: 1.125rem;
	}

	fieldset {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		margin: 0;
		padding: 0.75rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 20%, transparent);
		border-radius: var(--radius-field);
	}

	legend {
		padding: 0 0.3rem;
		font-size: 0.75rem;
		font-weight: 700;
	}

	.field {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.8125rem;
		font-weight: 700;
	}

	.field input {
		flex: 1 1 10rem;
		padding: 0.3rem 0.6rem;
		border: 2px solid color-mix(in srgb, var(--color-base-content) 25%, transparent);
		border-radius: var(--radius-field);
		background: var(--color-base-100);
		color: inherit;
		font: inherit;
		font-weight: 400;
	}

	.field input:focus {
		border-color: var(--color-base-content);
		outline: none;
	}

	.checkbox {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
		font-size: 0.8125rem;
	}

	.checkbox input {
		accent-color: var(--color-base-content);
	}

	.secret-reveal code {
		padding: 0.6rem;
		border: 2px dashed var(--color-base-content);
		border-radius: var(--radius-field);
		word-break: break-all;
	}

	.actions {
		display: flex;
		gap: 0.5rem;
	}

	.state-warning {
		margin: 0;
		color: var(--color-warning, #d97706);
		font-weight: 700;
		font-size: 0.8125rem;
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

	.state-error {
		margin: 0;
		color: var(--color-error, #dc2626);
		font-weight: 700;
		font-size: 0.8125rem;
	}

	.dim {
		color: color-mix(in srgb, var(--color-base-content) 65%, transparent);
		font-family: var(--font-sans);
		font-size: 0.75rem;
	}
</style>
