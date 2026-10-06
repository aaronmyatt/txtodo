// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { CHORD_MS, Chords, keyName } from "./keys";

const press = (key: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {}) =>
	keyName({ key, ctrlKey: false, metaKey: false, altKey: false, ...mods });

describe("keyName", () => {
	it("names printable keys as themselves and joins modifiers", () => {
		expect(press("/")).toBe("/");
		expect(press("?")).toBe("?");
		expect(press(" ")).toBe("Space");
		expect(press("k", { ctrlKey: true })).toBe("Ctrl-k");
		expect(press("Shift")).toBeNull();
	});
});

describe("Chords", () => {
	it("runs a single key at once and waits on the first key of a chord", () => {
		const chords = new Chords();
		expect(chords.resolve("/", 0)).toEqual({ kind: "command", id: "search.focus" });
		expect(chords.resolve("g", 10)).toEqual({ kind: "pending" });
		expect(chords.resolve("u", 20)).toEqual({ kind: "command", id: "nav.universal" });
	});

	it("drops a chord that waited too long or got a key it does not take", () => {
		const chords = new Chords();
		chords.resolve("g", 0);
		expect(chords.resolve("t", CHORD_MS + 1)).toEqual({ kind: "none" });
		chords.resolve("g", 0);
		expect(chords.resolve("/", 5)).toEqual({ kind: "command", id: "search.focus" });
		expect(chords.resolve("x", 6)).toEqual({ kind: "none" });
	});
});
