import { describe, expect, it } from "vitest";
import { activeSection, SECTIONS, sectionMatches } from "./sections";

describe("Settings sections", () => {
	const tokens = SECTIONS.find((s) => s.id === "tokens")!;

	it("matches a card by title or keyword, every term", () => {
		expect(sectionMatches(tokens, "Tok")).toBe(true);
		expect(sectionMatches(tokens, "secret scope")).toBe(true);
		expect(sectionMatches(tokens, "secret theme")).toBe(false);
		expect(sectionMatches(tokens, "")).toBe(true);
	});

	it("highlights the last card passed, and the last card at the end", () => {
		const tops = [
			{ id: "general", top: -300 },
			{ id: "appearance", top: 40 },
			{ id: "tokens", top: 600 }
		];
		expect(activeSection(tops, 120, false)).toBe("appearance");
		expect(activeSection(tops, 0, false)).toBe("general");
		expect(activeSection(tops, 0, true)).toBe("tokens");
		expect(activeSection([], 0, false)).toBeNull();
	});
});
