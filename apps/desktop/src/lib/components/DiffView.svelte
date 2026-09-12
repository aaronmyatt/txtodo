<script lang="ts">
	// Char-level diff between `mine` and `theirs` (plan M7, design §4.2/§4.7) rendered via the
	// shared wasm core `diff_text` binding — see $lib/wasmCore.ts, which owns the one-time wasm
	// `init()` this component doesn't need to know about.
	//
	// Plan §3.3 ("colours are never the only signal: ... errors have text") applies here too: a
	// deleted run uses <del> (semantic + strikethrough) plus a "−" glyph, an inserted run uses
	// <ins> (semantic + underline) plus a "+" glyph, so colour-blind users and screen readers
	// both get a non-colour signal.
	// Ref (semantics): https://developer.mozilla.org/en-US/docs/Web/HTML/Element/ins
	// Ref (semantics): https://developer.mozilla.org/en-US/docs/Web/HTML/Element/del
	import { diffText, type DiffSegment } from "$lib/wasmCore";

	let { mine, theirs }: { mine: string; theirs: string } = $props();

	let segments = $state<DiffSegment[]>([]);
	let error = $state("");

	async function run(a: string, b: string) {
		error = "";
		try {
			segments = await diffText(a, b);
		} catch (e) {
			error = String(e);
		}
	}

	// Re-diff whenever the pair changes (a new flag, or the sheet paging to the next one).
	$effect(() => {
		run(mine, theirs);
	});
</script>

<div class="diff" aria-label="Character-level difference between your edit and theirs">
	{#if error}
		<p class="error" role="alert">{error}</p>
	{:else}
		<p class="diff-text">
			{#each segments as seg, i (i)}
				{#if seg.op === "equal"}
					<span>{seg.text}</span>
				{:else if seg.op === "delete"}
					<del class="del" title="removed on your side"
						><span aria-hidden="true">&minus;</span>{seg.text}</del
					>
				{:else if seg.op === "insert"}
					<ins class="ins" title="added on their side"
						><span aria-hidden="true">+</span>{seg.text}</ins
					>
				{/if}
			{/each}
		</p>
	{/if}
</div>

<style>
	.diff-text {
		white-space: pre-wrap;
		font-family: ui-monospace, Menlo, monospace;
		line-height: 1.6;
	}

	.del {
		background: color-mix(in srgb, red 20%, transparent);
		text-decoration: line-through;
	}

	.ins {
		background: color-mix(in srgb, green 20%, transparent);
		text-decoration: underline;
	}

	.del span,
	.ins span {
		font-weight: 700;
		margin-inline-end: 0.15em;
	}

	.error {
		color: #b91c1c;
	}
</style>
