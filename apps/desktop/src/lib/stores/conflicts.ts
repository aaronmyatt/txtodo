// Pending needs_review conflicts (plan M7, design §4.7): a small Svelte store fed by the
// daemon's `Watch` stream (`daemon-change` events, see $lib/daemon.ts) and reconciled against
// `list_conflicts`, the daemon's own source of truth. Keyed by `task_id` — a ULID, unique across
// the whole workspace (crates/txtodo-model), so one flat map covers every open document.
// Pure/no Tauri imports on purpose: this module is plain store logic so it can be unit-tested
// without mocking `invoke`/`listen` (see conflicts.test.ts).
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { derived, writable, type Readable } from "svelte/store";
import type { ReviewFlag } from "$lib/daemon";

/** A `ReviewFlag` plus the workspace-relative path it belongs to. `ReviewFlagDto` itself carries
 * no path — it's always scoped to one `list_conflicts(path)` call or one `Change.path` — so this
 * store attaches it when a flag comes in. */
export interface PendingConflict extends ReviewFlag {
	path: string;
}

type ConflictMap = Map<string, PendingConflict>;

function createConflictsStore() {
	const { subscribe, update, set } = writable<ConflictMap>(new Map());

	return {
		subscribe,

		/** Merges flags carried by one `Watch`/`Change` event into the store. Additive only: a
		 * flag that has since been resolved elsewhere is removed via `remove`/`replaceForPath`,
		 * never simply by being absent from a later `addFlags` call. */
		addFlags(path: string, flags: ReviewFlag[]): void {
			if (flags.length === 0) return;
			update((map) => {
				const next = new Map(map);
				for (const flag of flags) next.set(flag.task_id, { ...flag, path });
				return next;
			});
		},

		/** Drops one flag after a *successful daemon resolve* — never call this for a dismiss.
		 * Dismissing the banner is a visibility toggle the component owns entirely on its own; the
		 * flag stays here (and in `list_conflicts`) until a real resolve op clears it server-side
		 * (design §4.7 invariant: "dismissing the banner or navigating away never clears a flag"). */
		remove(taskId: string): void {
			update((map) => {
				if (!map.has(taskId)) return map;
				const next = new Map(map);
				next.delete(taskId);
				return next;
			});
		},

		/** Replaces every flag for `path` with what `list_conflicts` reports right now. Callers
		 * (e.g. `ConflictBanner` on mount) use this to true up the store against the daemon, which
		 * is authoritative — the banner's count must never drift from `list_conflicts`. */
		replaceForPath(path: string, flags: ReviewFlag[]): void {
			update((map) => {
				const next = new Map(map);
				for (const [id, existing] of map) if (existing.path === path) next.delete(id);
				for (const flag of flags) next.set(flag.task_id, { ...flag, path });
				return next;
			});
		},

		/** Test/reset hook. */
		clear(): void {
			set(new Map());
		}
	};
}

export const pendingConflicts = createConflictsStore();

/** Pending flags for one open document, in a stable (line-number) order — `list_conflicts`'s own
 * order isn't documented, and `Map` iteration order is insertion order, neither of which is a
 * review order a human should rely on. */
export function flagsForPath(map: ConflictMap, path: string): PendingConflict[] {
	return [...map.values()]
		.filter((flag) => flag.path === path)
		.sort((a, b) => a.line_number - b.line_number);
}

/** Total pending count across every open document. */
export const pendingConflictCount: Readable<number> = derived(
	pendingConflicts,
	(map) => map.size
);

/**
 * Best-effort detection of design §4.7's `delete | edit → edit wins, task resurrected` row: an
 * empty/blank `mine` (this device tombstoned the task) against a populated `theirs` (the peer
 * edited it).
 *
 * IMPORTANT — this is speculative, and as of this writing it should never actually fire.
 * `ReviewFlagDto` (apps/desktop/src-tauri/src/dto.rs) carries no dedicated "resurrected" field,
 * and more fundamentally the daemon does not raise a `needs_review` flag for a delete/edit
 * conflict at all today: `crates/txtodo-crdt/src/review.rs::detect` only compares concurrent
 * *text* edits to the same task's description, and `Deleted` is an independent CRDT register that
 * never enters that comparison. `crates/txtodo-crdt/tests/conflicts.rs::delete_vs_edit_resurrects_the_task`
 * asserts this directly, with a doc comment that says outright: "Grepping the codebase for
 * 'resurrect' finds nothing... this asserts today's real behaviour... rather than the row's
 * wording, which describes a policy that has not been built yet". A real same-word-edit flag also
 * has non-empty text on *both* sides by construction (`review::detect`'s own `debug_assert`
 * requires `mine != theirs` unless both are empty), so this predicate is here purely for
 * forward-compatibility: if the daemon ever starts raising a flag for the delete/edit case, the
 * UI already knows how to word it.
 */
export function isResurrectCandidate(flag: ReviewFlag): boolean {
	return flag.mine.trim().length === 0 && flag.theirs.trim().length > 0;
}
