// The search matcher (core's, through wasm) and the hit ranges around it. The wasm core is loaded
// with `initSync` from the committed .wasm file: vitest runs in Node, where `init()` would fetch.
// Vitest: https://vitest.dev/api/
import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { initSync } from "../../wasm-core/txtodo_ffi";
import { markWasmLoaded } from "../../wasmCore";
import { hitRanges, matchesQuery } from "../query";

beforeAll(() => {
	initSync({ module: readFileSync(new URL("../../wasm-core/txtodo_ffi_bg.wasm", import.meta.url)) });
	markWasmLoaded();
});

describe("matchesQuery", () => {
	const table: [string, string, boolean][] = [
		["(A) Call Mum @phone", "mum @PHONE", true],
		["(A) Call Mum @phone", "mum -phone", false],
		["(A) Call Mum @phone", "", true],
		["x 2026-10-06 file taxes", "is:done", true],
		["x 2026-10-06 file taxes", "is:open", false],
		["file taxes", "taxes -x", false],
		["", "anything", false]
	];
	for (const [line, query, want] of table) {
		it(`${JSON.stringify(query)} on ${JSON.stringify(line)} is ${want}`, () => {
			expect(matchesQuery(line, query)).toBe(want);
		});
	}
});

describe("hitRanges", () => {
	it("marks every occurrence of every shown term, any case, overlaps merged", () => {
		expect(hitRanges("Call Mum, call MUM", "mum")).toEqual([
			[5, 8],
			[15, 18]
		]);
		expect(hitRanges("ferns first", "fern ferns")).toEqual([[0, 5]]);
	});

	it("shows nothing for exclusions and operators", () => {
		expect(hitRanges("call mum", "-mum is:open")).toEqual([]);
	});
});
