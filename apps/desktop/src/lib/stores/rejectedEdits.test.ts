// Unit test for the rejected-edit store (root todo id:01M2WK5DQQ9A3V3M4F2CJN0KMX): the daemon
// refused an edit while FileView was switching files or unmounting, and the text must survive.
// Vitest: https://vitest.dev/api/
import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { rejectedEdits } from "./rejectedEdits";

const A = "/ws/a";
const B = "/ws/b";

describe("rejectedEdits", () => {
	beforeEach(() => rejectedEdits.reset());

	it("keeps the refused text and the reason", () => {
		rejectedEdits.record({ workspace: A, path: "todo.txt", text: "buy milk", error: "line under review" });
		expect(get(rejectedEdits)).toEqual([
			{ workspace: A, path: "todo.txt", text: "buy milk", error: "line under review" }
		]);
	});

	it("keeps one entry per file, newest text winning", () => {
		rejectedEdits.record({ workspace: A, path: "todo.txt", text: "first", error: "a" });
		rejectedEdits.record({ workspace: A, path: "q4/todo.txt", text: "other file", error: "b" });
		rejectedEdits.record({ workspace: A, path: "todo.txt", text: "second", error: "c" });
		expect(get(rejectedEdits).map((e) => [e.path, e.text])).toEqual([
			["q4/todo.txt", "other file"],
			["todo.txt", "second"]
		]);
	});

	it("clears only the named path", () => {
		rejectedEdits.record({ workspace: A, path: "a/todo.txt", text: "x", error: "e" });
		rejectedEdits.record({ workspace: A, path: "b/todo.txt", text: "y", error: "e" });
		rejectedEdits.clear(A, "a/todo.txt");
		expect(get(rejectedEdits).map((e) => e.path)).toEqual(["b/todo.txt"]);
	});

	// Code review 2026-09-20, finding 4: every workspace has a todo.txt.
	it("a good save of todo.txt in one workspace leaves another workspace's refused todo.txt alone", () => {
		rejectedEdits.record({ workspace: A, path: "todo.txt", text: "typed in A", error: "workspace-changed:" });
		rejectedEdits.clear(B, "todo.txt"); // what FileView does after a successful save in B
		expect(get(rejectedEdits).map((e) => [e.workspace, e.text])).toEqual([[A, "typed in A"]]);
		rejectedEdits.record({ workspace: B, path: "todo.txt", text: "typed in B", error: "e" });
		expect(get(rejectedEdits)).toHaveLength(2);
	});
});
