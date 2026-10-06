// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { DEFAULT_PREFS, parsePrefs } from "./editorPrefs";

describe("parsePrefs", () => {
	it("reads every field that is in range", () => {
		const raw = JSON.stringify({ fontSize: 13, lineHeight: 22, lineNumbers: false, lengthHint: false });
		expect(parsePrefs(raw)).toEqual({ fontSize: 13, lineHeight: 22, lineNumbers: false, lengthHint: false });
	});

	it("falls back field by field on anything wrong", () => {
		expect(parsePrefs(null)).toEqual(DEFAULT_PREFS);
		expect(parsePrefs("not json")).toEqual(DEFAULT_PREFS);
		const mixed = parsePrefs(JSON.stringify({ fontSize: 99, lineHeight: 20, lineNumbers: "yes" }));
		expect(mixed).toEqual({ ...DEFAULT_PREFS, lineHeight: 20 });
		expect(parsePrefs(JSON.stringify({ fontSize: 14.5 })).fontSize).toBe(DEFAULT_PREFS.fontSize);
	});
});
