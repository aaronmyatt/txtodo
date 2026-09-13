// Pure logic for raw mode (tasks/desktop-raw-mode, plan §3.2/§7 "stretch: raw mode Cmd/Ctrl+E
// through the reconciler"). No DOM, no CodeMirror, no Tauri — plain-function testable, same
// split as `./editPopoverLogic.ts` (component keeps the CM6/Tauri wiring; this module owns the
// decisions).
//
// Design invariant (design §7 thin clients): even in raw mode the UI never parses or writes
// todo.txt itself. `computeDelta` below turns a raw-buffer edit into the same intent-level
// `Mutation`s `$lib/daemon.ts::applyMutations` already sends for the edit popover and the
// conflict-review sheet's resolutions — the one reconcile path this app can reach from
// `apps/desktop` without a daemon/proto change (this slice's scope is `apps/desktop` only; see
// this task's "As built" note for why a dedicated whole-document external-edit RPC, closer to
// what `crates/txtodo-core/src/diff.rs`'s `diff_lines` does for a real file-watcher edit, is out
// of scope here). Submitting per-task mutations rather than one whole-string replace is also
// *safer*, not just smaller: it only touches the tasks the human actually changed, so it can
// never clobber an unrelated concurrent change the way a naive whole-file overwrite could — and a
// genuinely conflicting concurrent edit to the same task still goes through the exact per-task
// CRDT merge/`needs_review` path the edit popover and `ConflictReviewSheet` already exercise
// (`ConflictReviewSheet.svelte`'s own `resolve()` doc comment: "the same `Apply(Edit)` path the
// edit popover uses").
import type { Mutation } from "$lib/daemon";

const ID_TAG = /\bid:(\S+)/;

/** The `id:<value>` tag on one line, or `""` when it has none yet (mirrors `FileView.svelte`'s
 * `hoveredLineRef` helper — same regex, same "empty means untagged" convention as `TaskRef`). */
function taskIdOf(line: string): string {
	const match = ID_TAG.exec(line);
	return match ? match[1] : "";
}

/** todo.txt lines never embed a literal `\n`, so a plain split is exact; `""` splits to `[]`
 * (an empty document has zero lines, not one empty line). */
function splitLines(text: string): string[] {
	return text === "" ? [] : text.split("\n");
}

interface BaselineLine {
	lineNumber: number; // 1-based, matching `TaskRef.line_number`
	text: string;
}

/**
 * The minimal set of intent-level `Mutation`s that turns `baseline` (the buffer raw mode started
 * from — the last daemon-confirmed content) into `next` (the edited raw buffer): a line-level
 * delta, never a whole-string replace (notes.md: "the UI submits a delta, never the whole
 * string"). `[]` for an unchanged buffer (no `Apply` call, no op-log entry).
 *
 * Matching strategy:
 * - A line carrying an `id:` tag that already existed in `baseline` keeps its identity: changed
 *   text becomes one `Edit` keyed by that `TaskRef`; a baseline id never seen again in `next`
 *   becomes one `Delete`.
 * - An untagged line (including a blank line — design §2.6: blanks are entries) is matched
 *   positionally against the pool of not-yet-claimed untagged baseline lines with identical text,
 *   so retyping the buffer without actually changing an untagged line is never mistaken for a
 *   delete+add pair. An untagged baseline line left unclaimed at the end is a removed blank/
 *   untagged line, addressed by `line_number` alone (`TaskRef.task_id: ""`, same "no id yet"
 *   convention `TaskRef` already documents).
 * - Anything left in `next` that matched nothing becomes one `Add` (appended).
 *
 * Known limitation (this slice's scope only): `Mutation` has no "insert at position N" or
 * "reorder" primitive, so a brand-new line always lands appended rather than inserted in place,
 * and reordering existing id-tagged lines changes their text only, never their position. A real
 * whole-document external-edit reconcile (matching `diff_lines`'s id-aware `Move`, M1 notes)
 * would need a new daemon RPC — out of scope for an `apps/desktop`-only slice.
 */
export function computeDelta(baseline: string, next: string): Mutation[] {
	if (baseline === next) return [];

	const baseById = new Map<string, BaselineLine>();
	const baseUntagged: BaselineLine[] = [];
	splitLines(baseline).forEach((text, i) => {
		const id = taskIdOf(text);
		const entry: BaselineLine = { lineNumber: i + 1, text };
		if (id) baseById.set(id, entry);
		else baseUntagged.push(entry);
	});

	const claimedIds = new Set<string>();
	const usedUntagged = new Set<number>();
	const mutations: Mutation[] = [];

	for (const text of splitLines(next)) {
		const id = taskIdOf(text);
		const base = id ? baseById.get(id) : undefined;
		if (id && base) {
			claimedIds.add(id);
			if (base.text !== text) {
				mutations.push({ kind: "edit", task: { line_number: base.lineNumber, task_id: id }, new_line: text });
			}
			continue;
		}

		// No id (or an id the baseline never had): try to match an unclaimed untagged baseline
		// line with identical text first, so an untouched untagged line never looks like churn
		// just because other lines around it moved.
		const matchIndex = baseUntagged.findIndex((entry, idx) => !usedUntagged.has(idx) && entry.text === text);
		if (matchIndex !== -1) {
			usedUntagged.add(matchIndex);
			continue;
		}

		mutations.push({ kind: "add", line: text });
	}

	for (const [id, entry] of baseById) {
		if (!claimedIds.has(id)) {
			mutations.push({
				kind: "delete",
				task: { line_number: entry.lineNumber, task_id: id },
				leave_blank: false
			});
		}
	}
	baseUntagged.forEach((entry, idx) => {
		if (!usedUntagged.has(idx)) {
			mutations.push({
				kind: "delete",
				task: { line_number: entry.lineNumber, task_id: "" },
				leave_blank: false
			});
		}
	});

	return mutations;
}

/** `true` when a raw-mode save would be a no-op: identical buffer, no `Apply`, no op-log entry
 * (notes.md "Edge cases & invariants" — same rule its own pseudocode spells out as
 * `if (next === lastFromWatch) return;`). Exported separately from `computeDelta` so a caller can
 * skip even building the delta for the common "nothing changed" case. */
export function isNoOpSave(baseline: string, next: string): boolean {
	return baseline === next;
}

/** Whether raw mode may be entered right now. The document raw mode edits is the *reconciled
 * projection*, so a pending `needs_review` flag means it isn't stable yet — resolve conflicts
 * first, then edit (notes.md "Edge cases & invariants"). */
export function canEnterRawMode(hasPendingReview: boolean): boolean {
	return !hasPendingReview;
}
