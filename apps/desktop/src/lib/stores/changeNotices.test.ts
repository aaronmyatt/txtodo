import { describe, expect, it } from "vitest";
import { readStoredPreference } from "./changeNotices";

describe("changeNotices", () => {
	it("is on unless turned off", () => {
		// No localStorage under vitest's node environment: the default.
		expect(readStoredPreference()).toBe(true);
	});
});
