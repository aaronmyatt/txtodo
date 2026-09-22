// The detail view's section branches (task desktop-notes-hidden), tested as the pure decisions
// `DetailView.svelte` reads rather than by mounting the component.
import { describe, expect, it } from "vitest";
import { notesLayout, notesMode, subListMode } from "../detailSections";

describe("subListMode", () => {
	it("shows the list when it has tasks and the start input otherwise", () => {
		expect(subListMode(3)).toBe("tasks");
		expect(subListMode(0)).toBe("start");
	});
});

describe("notesLayout", () => {
	it("a sub-list with tasks plus notes renders both, notes collapsed under the list", () => {
		expect(notesLayout(2, "Q4 goals\n")).toEqual({ collapsible: true, open: true });
	});

	it("a sub-list with tasks and empty notes collapses the notes closed", () => {
		expect(notesLayout(2, "  \n")).toEqual({ collapsible: true, open: false });
	});

	it("an empty sub-list renders the notes plain, always open", () => {
		expect(notesLayout(0, "")).toEqual({ collapsible: false, open: true });
		expect(notesLayout(0, "text")).toEqual({ collapsible: false, open: true });
	});
});

describe("notesMode", () => {
	const base = { parentLine: "(A) plan ref:q4", parentTaskId: "01ARZ3NDEKTSV4RRFFQ69G5FA2", hasRefTag: true, notesPathMissing: false };

	it("is loading until the parent line arrives", () => {
		expect(notesMode({ ...base, parentLine: "" })).toBe("loading");
	});

	it("needs a task id before it can address notes", () => {
		expect(notesMode({ ...base, parentTaskId: "" })).toBe("no-task-id");
	});

	it("is unavailable when the daemon found no notes path for a line that has a ref:", () => {
		expect(notesMode({ ...base, notesPathMissing: true })).toBe("unavailable");
	});

	it("shows the editor for a task with no ref: yet, even with no path (lazy creation)", () => {
		expect(notesMode({ ...base, hasRefTag: false, notesPathMissing: true })).toBe("editor");
		expect(notesMode(base)).toBe("editor");
	});
});
