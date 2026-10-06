// The search query (task desktop-ui-revamp; manifest search.*): whitespace-separated terms, all of
// which must hold, case-insensitive substring; `is:open` / `is:done`; a leading `-term` excludes.
// Matching is the shared core's (`txtodo_core::query::matches`, through the wasm build), the same
// code `txtodo list`, MCP and the TUI run, so every client finds the same lines. This file only
// adds what is display: where in a line the hits are.
import { matches_query } from "../wasm-core/txtodo_ffi";
import { isWasmLoaded } from "../wasmCore";

/** Whether `line` satisfies `query`. False until the wasm core has loaded (see `wasmReady`). */
export function matchesQuery(line: string, query: string): boolean {
	if (!isWasmLoaded()) return false;
	return matches_query(line, query);
}

/** Whether `query` has any term at all. */
export function isActiveQuery(query: string): boolean {
	return query.trim() !== "";
}

/** The terms whose text a hit shows: not `-term`, not an `is:` operator. Lowercased. */
export function highlightTerms(query: string): string[] {
	return query
		.split(/\s+/)
		.filter((t) => t !== "" && !t.startsWith("-") && !/^is:(open|done)$/i.test(t))
		.map((t) => t.toLowerCase());
}

/** The `[from, to)` character ranges of `line` that a hit marks: every case-insensitive
 * occurrence of a highlight term, overlaps merged, in order. */
export function hitRanges(line: string, query: string): [number, number][] {
	const hay = line.toLowerCase();
	const found: [number, number][] = [];
	for (const term of highlightTerms(query)) {
		for (let at = hay.indexOf(term); at !== -1; at = hay.indexOf(term, at + term.length)) {
			found.push([at, at + term.length]);
		}
	}
	found.sort((a, b) => a[0] - b[0]);
	const merged: [number, number][] = [];
	for (const r of found) {
		const last = merged.at(-1);
		if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
		else merged.push([...r]);
	}
	return merged;
}
