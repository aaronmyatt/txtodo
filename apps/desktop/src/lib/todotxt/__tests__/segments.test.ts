import { describe, expect, it } from "vitest";
import { segments } from "../segments";

describe("segments", () => {
	it("colours projects and contexts and keeps every character", () => {
		const text = "Ship it +work @home due:2026-10-09";
		const segs = segments(text);
		expect(segs.map((s) => s.text).join("")).toBe(text);
		expect(segs.find((s) => s.text === "+work")?.cls).toContain("tok-project");
		expect(segs.find((s) => s.text === "@home")?.cls).toContain("tok-context");
	});
});
