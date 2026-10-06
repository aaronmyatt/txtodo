// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { keepRefTag } from "../keepRefTag";

describe("keepRefTag", () => {
	it("keeps the original ref: when an edit dropped or mangled it", () => {
		const original = "plan the trip ref:plan-the-trip";
		expect(keepRefTag(original, "plan the big trip")).toBe("plan the big trip ref:plan-the-trip");
		expect(keepRefTag(original, "plan the trip ref:plan-th")).toBe("plan the trip ref:plan-the-trip");
	});

	it("leaves an intact ref: where it is", () => {
		expect(keepRefTag("go ref:trip +home", "go now ref:trip +home")).toBe("go now ref:trip +home");
	});

	it("leaves a line that had no ref: alone", () => {
		expect(keepRefTag("call mum", "call mum ref:x")).toBe("call mum ref:x");
	});
});
