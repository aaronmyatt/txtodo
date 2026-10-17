// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import type { FileInfo } from "../../daemon";
import { copyText, otherLists } from "../moveCopy";

const file = (path: string, kind = "FILE_KIND_TODO"): FileInfo => ({ path, kind, hash: "", done: 0, total: 0 });

describe("otherLists", () => {
	it("names every other list, the root first, and no notes file", () => {
		const files = [
			file("tasks/trip/todo.txt"),
			file("tasks/trip/notes.md", "FILE_KIND_NOTES"),
			file("todo.txt"),
			file("tasks/a/b/todo.txt"),
			file("tasks/inbox/todo.txt", "todo")
		];
		expect(otherLists(files, "tasks/trip/todo.txt")).toEqual(["todo.txt", "tasks/inbox/todo.txt", "tasks/a/b/todo.txt"]);
		expect(otherLists([file("todo.txt")], "todo.txt")).toEqual([]);
	});
});

describe("copyText", () => {
	it("drops the id and the ref and keeps the rest", () => {
		expect(copyText("(A) 2026-10-17 plan trip +home ref:trip id:01ARZ3NDEKTSV4RRFFQ69G5FAV due:2026-11-01")).toBe(
			"(A) 2026-10-17 plan trip +home due:2026-11-01"
		);
		expect(copyText("buy milk @shop")).toBe("buy milk @shop");
	});
});
