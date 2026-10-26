import { describe, expect, it } from "vitest";
import { droppedNote, wasDropped } from "./removal";

const row = (removed: boolean, removed_reason: string) => ({ removed, removed_reason });

describe("wasDropped", () => {
	it("is true only for a removed row the group dropped", () => {
		expect(wasDropped(row(true, "pruned_quiet"))).toBe(true);
		expect(wasDropped(row(true, "pruned_parked"))).toBe(true);
		expect(wasDropped(row(true, "left_out"))).toBe(true);
		expect(wasDropped(row(true, "manual"))).toBe(false);
		expect(wasDropped(row(true, ""))).toBe(false);
		expect(wasDropped(row(true, "from_the_future"))).toBe(false);
		expect(wasDropped(row(false, "pruned_quiet"))).toBe(false);
	});

	it("does not mistake an Object prototype key for a reason", () => {
		expect(wasDropped(row(true, "toString"))).toBe(false);
		expect(wasDropped(row(true, "constructor"))).toBe(false);
	});
});

describe("droppedNote", () => {
	it("says why and how to come back", () => {
		expect(droppedNote(row(true, "pruned_quiet"))).toBe("Pruned: not heard from for too long. Pair it again to bring it back.");
		expect(droppedNote(row(true, "left_out"))).toContain("another of your devices");
	});

	it("is empty when nobody dropped it", () => {
		expect(droppedNote(row(true, "manual"))).toBe("");
		expect(droppedNote(row(false, ""))).toBe("");
	});
});
