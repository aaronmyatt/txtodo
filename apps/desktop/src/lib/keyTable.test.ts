import { describe, expect, it } from "vitest";
import { keyGroups, keyLabel } from "./keyTable";
import type { Action } from "./parity/manifest";

const row = (over: Partial<Action>): Action => ({
	id: "x.y",
	title: "Do it",
	keys: [],
	scope: "global",
	desktop: { status: "done" },
	tui: { status: "done" },
	deviation: "",
	...over
});

describe("keyGroups", () => {
	it("takes built rows with desktop keys, by scope, with their own words", () => {
		const groups = keyGroups([
			row({ id: "nav.tasks", title: "Go to Tasks", keys: ["g t"] }),
			row({ id: "list.down", title: "Next line", keys: ["j", "Down"], scope: "list" }),
			row({ id: "x.planned", keys: ["q"], desktop: { status: "planned" } }),
			row({ id: "x.none", keys: [] }),
			row({ id: "x.tui_keys", keys: ["Ctrl-w"], desktop: { status: "differs" } }),
			row({ id: "detail.open", title: "Open", keys: ["Enter"], scope: "list", desktop: { status: "differs", keys: ["Mod-Enter"] } })
		]);
		expect(groups.map((g) => g.title)).toEqual(["Anywhere", "Tasks list (Esc first, so nothing has the keyboard)"]);
		expect(groups[0].rows).toEqual([{ keys: [[["g"], ["t"]]], title: "Go to Tasks" }]);
		expect(groups[1].rows[1]).toEqual({ keys: [[["Mod", "Enter"]]], title: "Open" });
	});

	it("reads the bundled manifest and finds the shell's own keys in it", () => {
		const titles = keyGroups().flatMap((g) => g.rows.map((r) => r.title));
		expect(titles).toContain("Next line");
	});
});

describe("keyLabel", () => {
	it("uses symbols on a Mac and words elsewhere", () => {
		expect(keyLabel("Mod", true)).toBe("⌘");
		expect(keyLabel("Mod", false)).toBe("Ctrl");
		expect(keyLabel("Down", false)).toBe("↓");
		expect(keyLabel("g", false)).toBe("g");
	});
});
