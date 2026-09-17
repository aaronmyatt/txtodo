// Tests for the pure logic behind `ActivityTab.svelte` (task desktop-activity-cross-workspace).
import { describe, expect, it } from "vitest";
import { isAgent, shortRoot } from "../activityLogic";

describe("isAgent", () => {
	it("is true for an agent principal", () => {
		expect(isAgent("agent:claude@dev")).toBe(true);
	});

	it("is false for a human principal", () => {
		expect(isAgent("you@dev")).toBe(false);
	});

	it("is false for an external-device principal", () => {
		expect(isAgent("external@dev")).toBe(false);
	});
});

describe("shortRoot", () => {
	it("returns the last path segment", () => {
		expect(shortRoot("/Users/oya/Development/txtodo")).toBe("txtodo");
	});

	it("handles a trailing slash", () => {
		expect(shortRoot("/private/tmp/txtodo-e2e-second-y02NnQ/")).toBe("txtodo-e2e-second-y02NnQ");
	});

	it("falls back to the whole string when there's no slash", () => {
		expect(shortRoot("relative")).toBe("relative");
	});
});
