// Unit test for the rejected-edit store (root todo id:01M2WK5DQQ9A3V3M4F2CJN0KMX): the daemon
// refused an edit while FileView was switching files or unmounting, and the text must survive.
import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { rejectedEdits } from "./rejectedEdits";

describe("rejectedEdits", () => {
	beforeEach(() => rejectedEdits.reset());

	it("keeps the refused text and the reason", () => {
		rejectedEdits.record({ path: "todo.txt", text: "buy milk", error: "line under review" });
		expect(get(rejectedEdits)).toEqual([{ path: "todo.txt", text: "buy milk", error: "line under review" }]);
	});

	it("keeps one entry per path, newest text winning", () => {
		rejectedEdits.record({ path: "todo.txt", text: "first", error: "a" });
		rejectedEdits.record({ path: "q4/todo.txt", text: "other file", error: "b" });
		rejectedEdits.record({ path: "todo.txt", text: "second", error: "c" });
		expect(get(rejectedEdits).map((e) => [e.path, e.text])).toEqual([
			["q4/todo.txt", "other file"],
			["todo.txt", "second"]
		]);
	});

	it("clears only the named path", () => {
		rejectedEdits.record({ path: "a/todo.txt", text: "x", error: "e" });
		rejectedEdits.record({ path: "b/todo.txt", text: "y", error: "e" });
		rejectedEdits.clear("a/todo.txt");
		expect(get(rejectedEdits).map((e) => e.path)).toEqual(["b/todo.txt"]);
	});
});
