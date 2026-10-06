import { describe, expect, it } from "vitest";
import type { FileInfo } from "../daemon";
import { meterFor } from "./meter";

const file = (path: string, done: number, total: number): FileInfo => ({ path, hash: "", kind: "todo", done, total });

describe("meterFor", () => {
	it("reads the root list's done over total", () => {
		expect(meterFor([file("tasks/a/todo.txt", 9, 9), file("todo.txt", 1, 4)], "todo.txt")).toEqual({
			progress: 0.25,
			title: "1 of 4 done"
		});
	});

	it("draws nothing for an empty or unknown list", () => {
		expect(meterFor([file("todo.txt", 0, 0)], "todo.txt")).toEqual({ progress: 0, title: "No tasks yet" });
		expect(meterFor([], "todo.txt").progress).toBe(0);
	});
});
