// How the editor's buffer goes back to the daemon (task desktop-reorder-propagates). Pure: no
// DOM, no CodeMirror, no Tauri; `apply` is passed in, so vitest drives it with a fake.
//
// First choice is `Replace`: a whole-document compare-and-swap (proto `Replace`, daemon
// `replace.rs`). The daemon reconciles the new text like an external edit, so an untouched line
// keeps its identity and a moved line is a move. The per-line delta in `./rawMode.ts` cannot say
// "this line moved", and under Sidecar identity (no `id:` tag in the text) it turns every edited
// line into delete + append.
//
// `Replace` is refused when the document changed since `base.hash` was read. Then the per-line
// delta runs against the old baseline: it only touches the lines the human changed, so the other
// party's change survives. A reorder is lost on that path, once; the repaint shows it.
import type { ApplyResult, Mutation } from "$lib/daemon";
import { computeDelta } from "./rawMode";

/** The text the buffer was edited from and the hash `GetFile` gave with it (`""`: not known). */
export interface Baseline {
	text: string;
	hash: string;
}

export type ApplyFn = (path: string, mutations: Mutation[]) => Promise<ApplyResult>;

/** What a save did. `replace` carries the document's new hash: the buffer is now the baseline. */
export type SaveOutcome = { how: "none" } | { how: "replace"; hash: string } | { how: "delta" };

/** First word of a refused `apply` whose document moved under the caller. Mirrors
 * `desktop_lib::daemon::FAILED_PRECONDITION_TOKEN`; keep the two in step. */
export const FAILED_PRECONDITION_TOKEN = "failed-precondition:";

/** Whether `error` is the daemon saying "the document changed since you read it". */
export function isStaleBase(error: unknown): boolean {
	return String(error).includes(FAILED_PRECONDITION_TOKEN);
}

/**
 * `next` with the baseline's line ending. CodeMirror holds a document as lines and joins them
 * with `\n` (https://codemirror.net/docs/ref/#state.EditorState^lineSeparator), so a CRLF file
 * comes out of the editor as LF. A whole-document write must not change every line's ending.
 */
export function matchEndings(baseline: string, next: string): string {
	if (!baseline.includes("\r\n")) return next;
	// String.prototype.replace with a regex: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String/replace
	return next.replace(/\r?\n/g, "\r\n");
}

/**
 * `true` when `next` holds exactly the baseline's lines in another order: a line was moved and
 * nothing was typed. Such a change is safe to save at once; typed text waits for blur or Cmd-S.
 * A reorder never changes the length, so the cheap check runs first (this is called per edit).
 */
export function isReorderOnly(baseline: string, next: string): boolean {
	if (baseline === next || baseline.length !== next.length) return false;
	const a = baseline.split("\n").sort();
	const b = next.split("\n").sort();
	return a.length === b.length && a.every((line, i) => line === b[i]);
}

/** Saves `next` over `base`: `Replace` first, the per-line delta when the base is stale or not
 * known. Any other refusal is thrown to the caller unchanged. */
export async function saveBuffer(apply: ApplyFn, path: string, base: Baseline, next: string): Promise<SaveOutcome> {
	if (base.text === next) return { how: "none" };
	if (base.hash) {
		const contents = matchEndings(base.text, next);
		try {
			const result = await apply(path, [{ kind: "replace", base_hash: base.hash, contents }]);
			return { how: "replace", hash: result.hash };
		} catch (e) {
			if (!isStaleBase(e)) throw e;
		}
	}
	const mutations = computeDelta(base.text, next);
	if (mutations.length === 0) return { how: "none" };
	await apply(path, mutations);
	return { how: "delta" };
}
