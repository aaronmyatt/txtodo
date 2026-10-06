import { describe, expect, it } from "vitest";
import { moveLineText } from "../dragLines";

describe("moveLineText", () => {
	const text = "a\nb\nc\n";
	it("moves a line up, down and to the end, keeping the final newline", () => {
		expect(moveLineText(text, 3, 1)).toBe("c\na\nb\n");
		expect(moveLineText(text, 1, 3)).toBe("b\na\nc\n");
		expect(moveLineText(text, 1, 4)).toBe("b\nc\na\n");
		expect(moveLineText("a\nb", 1, 3)).toBe("b\na");
	});

	it("leaves the text alone for a drop on the line itself or right under it", () => {
		expect(moveLineText(text, 2, 2)).toBe(text);
		expect(moveLineText(text, 2, 3)).toBe(text);
		expect(moveLineText(text, 9, 1)).toBe(text);
	});
});
