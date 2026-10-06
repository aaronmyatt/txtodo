// The logo's geometry and its progress maths (task desktop-ui-revamp, `Mark.svelte`). The four
// strokes are assets/brand/tx-mark.svg's: the t stem, the t crossbar hooking down like a check,
// then the two strokes of the x. A progress of 0..1 draws them in that order.

/** The mark's strokes, in drawing order (viewBox 0 0 100 64, stroke 7, round caps). */
export const MARK_PATHS = [
	"M20 6 L20 42 C 20 47, 23 49, 28 49",
	"M8 20 L28 20 L40 34",
	"M58 22 L80 44",
	"M80 22 L58 44"
] as const;

/** A stroke's dash offset when hidden. With `pathLength=1` and `stroke-dasharray: 1 2`, an offset
 * of exactly 1 still paints the round cap as a dot; 1.05 moves it fully out of sight. */
export const HIDDEN = 1.05;

/** Each stroke's `stroke-dashoffset` for `progress` (clamped to 0..1): the first quarter draws the
 * stem, the next the crossbar, and so on; 0 hides all four, 1 shows the whole mark. */
export function markOffsets(progress: number): number[] {
	const p = Math.min(1, Math.max(0, Number.isFinite(progress) ? progress : 0));
	return MARK_PATHS.map((_, i) => {
		const drawn = Math.min(1, Math.max(0, p * MARK_PATHS.length - i));
		return drawn === 0 ? HIDDEN : 1 - drawn;
	});
}
