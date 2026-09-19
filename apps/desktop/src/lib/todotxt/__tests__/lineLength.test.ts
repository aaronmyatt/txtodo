// Vectors shared with crates/txtodo-core/src/line_length.rs — change one, change both.
import { describe, expect, it } from "vitest";
import { hintOffset, LINE_LENGTH_HINT, visibleLength } from "../lineLength";

const ID = "id:01J9K3H5Z7Q8X2M4N6P8R0T2V4"; // 29 chars, plus its 1 blank = the 30 the hint used to count

describe("visibleLength / hintOffset", () => {
	it("counts a plain line as its characters", () => {
		expect(visibleLength("a".repeat(100))).toBe(100);
		expect(hintOffset("a".repeat(100))).toBeNull();
		expect(hintOffset("a".repeat(101))).toBe(LINE_LENGTH_HINT);
	});

	it("does not count the line's own id tag or the blank before it", () => {
		const line = `${"a".repeat(75)} ${ID}`;
		expect(line.length).toBe(105);
		expect(visibleLength(line)).toBe(75);
		expect(hintOffset(line)).toBeNull();
	});

	it("starts the mark past the hint in visible characters, skipping a hidden tag before it", () => {
		const line = `${"a".repeat(50)} ${ID} ${"b".repeat(60)}`;
		expect(visibleLength(line)).toBe(50 + 1 + 60);
		// 50 a's, one blank, then 49 b's are the first 100 visible characters; the 50th b is past them.
		expect(hintOffset(line)).toBe(line.length - 60 + 49);
	});

	it("counts code points, not UTF-16 units or bytes", () => {
		expect(visibleLength("😀".repeat(60))).toBe(60); // 120 UTF-16 units
		expect(hintOffset("😀".repeat(60))).toBeNull();
		expect(visibleLength("字".repeat(60))).toBe(60); // 180 bytes
		expect(hintOffset("😀".repeat(101))).toBe(200); // UTF-16 index of the 101st emoji
	});

	it("keeps a malformed id-like word visible only if it is not `id:` plus text", () => {
		expect(visibleLength("a id:")).toBe(5); // no value, so not a tag
		expect(visibleLength("a idx:1")).toBe(7); // `\bid:` needs the word to be exactly `id`
	});
});
