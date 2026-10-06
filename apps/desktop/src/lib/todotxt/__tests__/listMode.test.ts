import { describe, expect, it } from "vitest";
import { docLines, listMutations, moveTarget, type ListDoc } from "../listMode";

const doc: ListDoc = {
	lines: docLines("(A) buy milk\ncall mum\nx 2026-10-05 pay rent\n"),
	taskIds: ["T1", "T2", "T3"]
};
const reopen = async (line: string) => line.replace(/^x \d{4}-\d{2}-\d{2} /, "");

describe("moveTarget", () => {
	it("moves within the list and stops at its edges", () => {
		expect(moveTarget("list.down", 1, doc)).toBe(2);
		expect(moveTarget("list.down", 3, doc)).toBe(3);
		expect(moveTarget("list.up", 1, doc)).toBe(1);
		expect(moveTarget("list.first", 3, doc)).toBe(1);
		expect(moveTarget("list.last", 1, doc)).toBe(3);
		expect(moveTarget("list.delete", 1, doc)).toBeNull();
	});
});

describe("listMutations", () => {
	it("completes an open line and reopens a done one", async () => {
		expect(await listMutations("list.toggle_complete", 2, doc, "2026-10-06", reopen)).toEqual({
			mutations: [{ kind: "complete", task: { line_number: 2, task_id: "T2" }, today: "2026-10-06" }],
			cursor: 2
		});
		expect((await listMutations("list.toggle_complete", 3, doc, "2026-10-06", reopen))?.mutations).toEqual([
			{ kind: "edit", task: { line_number: 3, task_id: "T3" }, new_line: "pay rent" }
		]);
	});

	it("moves by swapping with the neighbour, and not past an edge", async () => {
		expect(await listMutations("list.move_down", 1, doc, "", reopen)).toEqual({
			mutations: [{ kind: "move_before", task: { line_number: 2, task_id: "T2" }, before: { line_number: 1, task_id: "T1" } }],
			cursor: 2
		});
		expect((await listMutations("list.move_up", 2, doc, "", reopen))?.cursor).toBe(1);
		expect(await listMutations("list.move_up", 1, doc, "", reopen)).toBeNull();
		expect(await listMutations("list.move_down", 3, doc, "", reopen)).toBeNull();
	});

	it("deletes, keeping the cursor in the list", async () => {
		expect(await listMutations("list.delete", 3, doc, "", reopen)).toEqual({
			mutations: [{ kind: "delete", task: { line_number: 3, task_id: "T3" }, leave_blank: false }],
			cursor: 2
		});
	});
});
