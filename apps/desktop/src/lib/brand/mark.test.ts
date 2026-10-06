// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { HIDDEN, markOffsets } from "./mark";

describe("markOffsets", () => {
	it("hides every stroke at 0 and shows every stroke at 1", () => {
		expect(markOffsets(0)).toEqual([HIDDEN, HIDDEN, HIDDEN, HIDDEN]);
		expect(markOffsets(1)).toEqual([0, 0, 0, 0]);
	});

	it("draws the strokes in order, a quarter each", () => {
		expect(markOffsets(0.25)).toEqual([0, HIDDEN, HIDDEN, HIDDEN]);
		expect(markOffsets(0.375)).toEqual([0, 0.5, HIDDEN, HIDDEN]);
		expect(markOffsets(0.75)).toEqual([0, 0, 0, HIDDEN]);
	});

	it("clamps nonsense", () => {
		expect(markOffsets(-3)).toEqual(markOffsets(0));
		expect(markOffsets(7)).toEqual(markOffsets(1));
		expect(markOffsets(Number.NaN)).toEqual(markOffsets(0));
	});
});
