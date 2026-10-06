// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { docCounts } from "../counts";

describe("docCounts", () => {
	it("counts open and done tasks, skipping blanks, and every line", () => {
		const text = "(A) call mum\n\nx 2026-10-06 buy milk\nwalk dog\n";
		expect(docCounts(text)).toEqual({ open: 2, done: 1, lines: 4 });
	});

	it("is all zero for an empty document", () => {
		expect(docCounts("")).toEqual({ open: 0, done: 0, lines: 0 });
	});

	it("does not read a word starting with x as done", () => {
		expect(docCounts("xylophone lesson")).toEqual({ open: 1, done: 0, lines: 1 });
	});
});
