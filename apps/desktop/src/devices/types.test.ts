// Closed-union guard test: the token-create form must never be able to build a scope string the
// daemon doesn't recognize (design §6.2).
import { describe, expect, it } from "vitest";
import { BASE_SCOPES, isValidScope } from "./types";

describe("isValidScope", () => {
	it("accepts every base scope", () => {
		for (const scope of BASE_SCOPES) {
			expect(isValidScope(scope)).toBe(true);
		}
	});

	it("accepts a restrictor scope with a non-empty suffix", () => {
		expect(isValidScope("project:+work")).toBe(true);
		expect(isValidScope("context:@phone")).toBe(true);
		expect(isValidScope("file:work.txt")).toBe(true);
	});

	it("rejects a restrictor scope with an empty suffix", () => {
		expect(isValidScope("project:")).toBe(false);
		expect(isValidScope("context:")).toBe(false);
		expect(isValidScope("file:")).toBe(false);
	});

	it("rejects an unrecognized scope", () => {
		expect(isValidScope("write:everything")).toBe(false);
		expect(isValidScope("admin")).toBe(false);
		expect(isValidScope("")).toBe(false);
	});

	it("rejects a restrictor-looking prefix that isn't one of the three", () => {
		expect(isValidScope("tag:urgent")).toBe(false);
	});
});
