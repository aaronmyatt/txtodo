import { describe, expect, it } from "vitest";
import { newestChange, popChange, popIfNewest, recordChange } from "./changes";

describe("session changes", () => {
	it("keeps changes newest last, skips empty ones, and pops a toast's change only while newest", () => {
		const a = { path: "todo.txt", ops: 2, workspaceRoot: "" };
		const b = { path: "todo.txt", ops: 1, workspaceRoot: "" };
		recordChange(a);
		recordChange({ path: "todo.txt", ops: 0, workspaceRoot: "" });
		recordChange(b);
		expect(newestChange()).toBe(b);
		expect(popIfNewest(a)).toBe(false);
		expect(popChange()).toBe(b);
		expect(popIfNewest(a)).toBe(true);
		expect(popChange()).toBeUndefined();
	});
});
