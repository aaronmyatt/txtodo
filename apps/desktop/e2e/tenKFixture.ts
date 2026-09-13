// The single source of truth for the 10k-line todo.txt fixture used by BOTH the visual-regression
// main-view snapshot and the first-paint perf budget (tasks/desktop-visual-regression/notes.md:
// "one fixture, two consumers... so the perf number and the goldens describe the same document").
// A checked-in generator rather than a checked-in 10k-line text file: a generator is reviewable in
// a few lines, while a literal 10,000-line fixture file is not something a diff meaningfully shows.
//
// Deterministic on purpose: no `Math.random`, no `Date.now`, no ULID generation — every call
// produces byte-identical output, which is what lets goldens and the perf assertion both describe
// the same fixed document across runs and machines.
export const TEN_K_LINE_COUNT = 10_000;

/** What CM6's `EditorState.doc.lines` actually reports for this fixture, for the
 * `data-line-count` testability hook (FileView.svelte) that both the perf test and the main-view
 * snapshot wait on. `generateTenKLines()` ends the text with a trailing `\n` (the POSIX-text-file
 * convention every other fixture in this suite also follows), and CM6's `Text` splits on `\n` —
 * 10,000 newlines produce 10,001 line *segments*, the last one empty. Not a bug, just CM6's own
 * documented line-counting (https://codemirror.net/docs/ref/#state.Text.lines): keeping it as a
 * separate named constant (rather than quietly waiting on `10001` in two far-apart test files)
 * documents why the number isn't the same as `TEN_K_LINE_COUNT`. */
export const TEN_K_DOC_LINES = TEN_K_LINE_COUNT + 1;

/** The slug of the one `ref:` sub-list the generated file points at (line 3) — see
 * `seedTenKWorkspace` in `fixtures.ts`, which creates the matching sub-directory so the main
 * view's `n/m` progress decoration (`$lib/todotxt/decorations.ts::resolveRefIndicator`) has a
 * real, non-dangling target to resolve on the very first screen. */
export const TEN_K_REF_SLUG = "q1-goals";

/**
 * Generates the fixture's full text. The first handful of lines are hand-shaped so the viewport
 * (CM6 only ever paints `visibleRanges` — the reason this file can be 10k lines at all, see
 * `$lib/todotxt/decorations.ts`'s module doc) shows every decoration the snapshot must assert:
 * a hidden `id:` tag, a completed/struck line, and a resolvable `ref:` line's `n/m` indicator. The
 * remaining ~9,990 lines are a fixed, cheap-to-generate repeating pattern — their only job is to
 * be *there* for the perf budget, not to be individually meaningful.
 */
export function generateTenKLines(): string {
	// 26 chars total (real ULID length — matches the shape `e2e/fixtures.ts`'s other hand-seeded
	// fixtures use, e.g. "01ARZ3NDEKTSV4RRFFQ69G5FAV"), built as a fixed 9-char prefix plus the
	// line number zero-padded to 17 digits, so every id is unique and length-stable up to 10,000.
	const idFor = (n: number) => `id:01M2B4ZWA${String(n).padStart(17, "0")}`;

	const lines: string[] = [
		`(A) plan Q1 goals ref:${TEN_K_REF_SLUG} ${idFor(1)}`,
		`x 2026-01-01 2025-12-20 finished onboarding doc ${idFor(2)}`,
		`(B) buy milk +home @errand ${idFor(3)}`
	];
	for (let n = lines.length + 1; n <= TEN_K_LINE_COUNT; n++) {
		// Every 7th line completed, every 13th a plain low-priority task, else a mid-priority open
		// task — enough variety to exercise the done/strike decoration repeatedly through the
		// document without any randomness.
		if (n % 7 === 0) {
			lines.push(`x 2026-01-01 completed task ${n} +proj @ctx ${idFor(n)}`);
		} else if (n % 13 === 0) {
			lines.push(`(C) low priority task ${n} @ctx ${idFor(n)}`);
		} else {
			lines.push(`(B) task number ${n} +proj @ctx ${idFor(n)}`);
		}
	}
	return `${lines.join("\n")}\n`;
}
