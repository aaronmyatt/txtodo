// Duplicate-line helpers (ADR 0032). Ref (vitest): https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import type { DuplicateGroup } from "$lib/daemon";
import { copiesLabel, groupsLabel, keepMutations } from "./duplicates";

const group: DuplicateGroup = {
	tasks: [
		{ task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V1", line_number: 2 },
		{ task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V2", line_number: 5 },
		{ task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V3", line_number: 9 }
	]
};

const ids = (ms: ReturnType<typeof keepMutations>) =>
	ms.map((m) => (m.kind === "delete" ? m.task.task_id : `not a delete: ${m.kind}`));

describe("keepMutations", () => {
	it("keeping the newest deletes every older copy by task id, leaving no blank", () => {
		const ms = keepMutations(group, "newest");
		expect(ids(ms)).toEqual(["01J9K3H5Z7Q8X2M4N6P8R0T2V1", "01J9K3H5Z7Q8X2M4N6P8R0T2V2"]);
		expect(ms.every((m) => m.kind === "delete" && !m.leave_blank)).toBe(true);
		// By id alone: a line number would be resolved first and moves inside the batch.
		expect(ms.every((m) => m.kind === "delete" && m.task.line_number === 0)).toBe(true);
	});

	it("keeping the oldest deletes every newer copy", () => {
		expect(ids(keepMutations(group, "oldest"))).toEqual([
			"01J9K3H5Z7Q8X2M4N6P8R0T2V2",
			"01J9K3H5Z7Q8X2M4N6P8R0T2V3"
		]);
	});

	it("a group of one asks for nothing", () => {
		expect(keepMutations({ tasks: group.tasks.slice(0, 1) }, "newest")).toEqual([]);
	});
});

describe("labels", () => {
	it("names where the copies are, oldest and newest marked", () => {
		expect(copiesLabel(group)).toBe("lines 2 (oldest), 5 and 9 (newest)");
		expect(copiesLabel({ tasks: group.tasks.slice(1) })).toBe("lines 5 (oldest) and 9 (newest)");
	});

	it("counts groups in words", () => {
		expect(groupsLabel(1)).toBe("1 line is in the file twice");
		expect(groupsLabel(3)).toBe("3 lines are in the file more than once");
	});
});
