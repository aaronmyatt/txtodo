import { describe, expect, it } from "vitest";
import { keyGroups, keyLabel } from "./keyTable";

describe("keyGroups", () => {
	it("leads with the global hotkey and takes the shell keys from keys.ts", () => {
		const groups = keyGroups(["Super", "Shift", "Space"]);
		expect(groups[0]).toEqual({ title: "Anywhere", rows: [{ keys: [[["Super", "Shift", "Space"]]], title: "Quick Add from any app" }] });
		const goTo = groups.find((g) => g.title === "Go to")!;
		expect(goTo.rows[0]).toEqual({ keys: [[["g"], ["t"]]], title: "Go to Tasks" });
	});

	it("leaves the hotkey out when it is not known", () => {
		expect(keyGroups([])[0].title).toBe("Buffer");
	});

	it("splits a modified press but keeps a lone dash key", () => {
		const buffer = keyGroups([]).find((g) => g.title === "Buffer")!;
		expect(buffer.rows[0].keys).toEqual([[["Mod", "Enter"]]]);
	});
});

describe("keyLabel", () => {
	it("uses symbols on a Mac and words elsewhere", () => {
		expect(keyLabel("Mod", true)).toBe("⌘");
		expect(keyLabel("Mod", false)).toBe("Ctrl");
		expect(keyLabel("Enter", true)).toBe("↵");
		expect(keyLabel("g", false)).toBe("g");
	});
});
