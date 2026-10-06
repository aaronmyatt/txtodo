import { describe, expect, it } from "vitest";
import { markHits, segments } from "../segments";

describe("segments", () => {
	it("colours projects and contexts and keeps every character", () => {
		const text = "Ship it +work @home due:2026-10-09";
		const segs = segments(text);
		expect(segs.map((s) => s.text).join("")).toBe(text);
		expect(segs.find((s) => s.text === "+work")?.cls).toContain("tok-project");
		expect(segs.find((s) => s.text === "@home")?.cls).toContain("tok-context");
	});
});

describe("markHits", () => {
	it("splits segments at hit ranges, across a token boundary too", () => {
		const segs = [
			{ text: "buy ", cls: "" },
			{ text: "+milk", cls: "tok-project" }
		];
		expect(markHits(segs, [[2, 7]])).toEqual([
			{ text: "bu", cls: "" },
			{ text: "y ", cls: "", hit: true },
			{ text: "+mi", cls: "tok-project", hit: true },
			{ text: "lk", cls: "tok-project" }
		]);
	});

	it("leaves segments alone with no ranges", () => {
		const segs = [{ text: "a", cls: "" }];
		expect(markHits(segs, [])).toBe(segs);
	});
});
